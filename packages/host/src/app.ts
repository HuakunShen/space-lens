import { Hono } from 'hono'
import { basename } from 'node:path'

import type { Capabilities, DiscoveryRequest, Health, Session } from '@space-lens/contract'
import {
  AiChatRequestSchema,
  AppAuditReportSchema,
  AppAuditRequestSchema,
  AppIconRequestSchema,
  AuditLogSchema,
  CandidatesRequestSchema,
  ChildrenPageRequestSchema,
  CleanupExecuteRequestSchema,
  CleanupPlanRequestSchema,
  DiscoveryRequestSchema,
  FreeUpCandidatesSchema,
  HistoryRequestSchema,
  MemoryHistorySchema,
  MemorySnapshotSchema,
  RevealPathRequestSchema,
  ScanIdSchema,
  ScanStartRequestSchema,
  SessionExchangeRequestSchema,
  TerminateExecuteRequestSchema,
  TerminateOutcomeSchema,
  TerminatePlanRequestSchema,
  TerminatePlanSchema,
  TreeSliceRequestSchema,
} from '@space-lens/contract'
import type {
  AppAuditRequest,
  AiChatRequest,
  AiConfigView,
  CandidatesRequest,
  HistoryRequest,
  RevealAction,
  TerminateExecuteRequest,
  TerminatePlanRequest,
} from '@space-lens/contract'
import type { SessionRecord } from './auth.ts'
import type { AuthService } from './auth.ts'
import type { AssetHandler } from './assets.ts'
import type { ClientAllowlist } from './cidr.ts'
import { normalizeRemoteAddress } from './cidr.ts'
import type { EventRing } from './events.ts'
import { ProblemError, Problems } from './problems.ts'
import { authoritiesFor, checkOrigin, originsForAuthorities, type OriginPolicy } from './origins.ts'
import type { ResolvedServeConfig } from './config.ts'
import type { ScanManager } from './scan-store.ts'
import { mountedVolumes, volumeCapacities } from './volumes.ts'
import { resolveLocalScanBin } from './local-process.ts'
import { runAppAuditCli, runAppIconCli } from './app-audit.ts'
import { forwardChatCompletion, forwardChatStream } from './ai.ts'
import type { MemoryBridge, MemoryCall } from './memory.ts'
import { revealPath, type FileManagerTarget } from './reveal.ts'

export interface HostDeps {
  config: ResolvedServeConfig
  auth: AuthService
  scans: ScanManager
  events: EventRing
  assets: AssetHandler
  health: Health
  capabilities: Capabilities
  allowlist: ClientAllowlist
  /** Absolute protected release CLI path; otherwise env/repository discovery is used. */
  localScanBin?: string
  /**
   * Bridge to `spacelens memory stdio`. Present only when the host was started
   * with `--allow-memory` and the release CLI resolved; its absence turns every
   * `/api/v1/memory/*` route into the standard `Unavailable` problem.
   */
  memory?: MemoryBridge
  /**
   * How a revealed path reaches the file manager. Absent in production (the
   * platform launcher runs); tests inject a recorder so no test ever opens a
   * real window.
   */
  revealLauncher?: (target: FileManagerTarget) => void | Promise<void>
}

type AppEnv = { Variables: { session: SessionRecord } }

export type HonoApp = Hono<AppEnv> & { rebindGate: (port: number) => void }

const EXCHANGE_RATE_LIMIT = 10
const EXCHANGE_RATE_WINDOW_MS = 60_000

function frame(event: string, id: number, data: unknown): string {
  return `retry: 3000\nevent: ${event}\nid: ${id}\ndata: ${JSON.stringify(data)}\n\n`
}

function jsonResponse(payload: unknown, status: number, extraHeaders?: Record<string, string>): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: { 'content-type': 'application/json; charset=utf-8', ...extraHeaders },
  })
}

export function buildApp(deps: HostDeps): HonoApp {
  const app = new Hono<AppEnv>() as HonoApp
  const { config, auth, scans, events, assets, health, capabilities, allowlist, localScanBin, memory } = deps

  // App-storage audit spawns share one per-process guard: at most two
  // concurrent CLI audits; the third caller gets a 429 LimitExceeded.
  let activeAppAudits = 0
  const MAX_CONCURRENT_APP_AUDITS = 2

  // The gate is built from the listener once the real port is known — never
  // from a request. Until rebindGate is called (right after bind), a
  // conservative placeholder stands.
  let policy: OriginPolicy = {
    authorities: authoritiesFor(config.bind.address, config.port),
    allowedOrigins: [...config.uiOrigins],
  }
  app.rebindGate = (port: number) => {
    const authorities = authoritiesFor(config.bind.address, port)
    policy = { authorities, allowedOrigins: [...originsForAuthorities(authorities), ...config.uiOrigins] }
  }

  const remoteAddress = (c: { env: unknown; req: { header: (name: string) => string | undefined } }): string | null => {
    const forwarded = c.req.header('x-forwarded-for')
    if (forwarded !== undefined && config.trustProxy) {
      const first = forwarded.split(',')[0]?.trim()
      if (first) return first
    }
    const env = c.env as { incoming?: { socket?: { remoteAddress?: string } } } | undefined
    return env?.incoming?.socket?.remoteAddress ?? null
  }

  const corsHeadersFor = (origin: string | null): Record<string, string> => {
    if (origin === null || !config.uiOrigins.includes(origin)) return {}
    return {
      'access-control-allow-origin': origin,
      'access-control-allow-methods': 'GET, POST, OPTIONS',
      'access-control-allow-headers': 'Authorization, Content-Type',
      vary: 'Origin',
    }
  }

  // 1. Gate every request: client network, then Host/Origin/Sec-Fetch-Site.
  app.use('*', async (c, next) => {
    const origin = c.req.header('origin') ?? null
    // CORS headers are decided before any check so refusals stay readable by
    // the allowed UI that made the request.
    for (const [name, value] of Object.entries(corsHeadersFor(origin))) c.header(name, value)

    const remote = remoteAddress(c)
    if (remote !== null && !allowlist.check(remote)) {
      throw Problems.forbidden(`client address ${normalizeRemoteAddress(remote)} is not in the allowed networks`)
    }
    const verdict = checkOrigin(
      {
        hostHeader: c.req.header('host') ?? null,
        origin,
        secFetchSite: c.req.header('sec-fetch-site') ?? null,
        secFetchMode: c.req.header('sec-fetch-mode') ?? null,
      },
      policy,
    )
    if (verdict !== 'ok') {
      if (verdict === 'refused-host')
        throw new ProblemError(
          {
            code: 'InvalidRequest',
            message: 'request is not addressed to this service (Host header refused)',
            retryable: false,
          },
          400,
        )
      if (verdict === 'refused-origin') throw Problems.forbidden('request origin is not allowed')
      throw Problems.forbidden('cross-site requests without a matching origin are refused')
    }
    await next()
  })

  // 2. Health: open, sparse, no user data.
  app.get('/health', (c) => c.json(health))

  // 3. Pairing exchange: open but rate-limited; single-use tickets only.
  const exchangeAttempts = new Map<string, { count: number; resetAt: number }>()
  app.post('/api/v1/session/exchange', async (c) => {
    const origin = c.req.header('origin') ?? ''
    const ip = remoteAddress(c) ?? ''
    const key = `${origin}|${ip}`
    const now = Date.now()
    const bucket = exchangeAttempts.get(key)
    if (bucket === undefined || bucket.resetAt <= now) {
      if (exchangeAttempts.size > 4096) exchangeAttempts.clear()
      exchangeAttempts.set(key, { count: 1, resetAt: now + EXCHANGE_RATE_WINDOW_MS })
    } else {
      bucket.count += 1
      if (bucket.count > EXCHANGE_RATE_LIMIT) throw Problems.limit('too many pairing attempts; wait a minute')
    }

    const raw = await c.req.json().catch(() => undefined)
    const parsed = SessionExchangeRequestSchema.safeParse(raw)
    if (!parsed.success) throw Problems.invalid('malformed pairing request')
    const result = auth.exchange(parsed.data.ticket, {
      origin: origin === '' ? null : origin,
      password: parsed.data.password,
    })
    if (!result.ok) {
      throw Problems.unauthenticated('ticket rejected (unknown, expired, origin-bound, or wrong password)')
    }
    const session: Session = {
      token: result.session.token,
      sessionId: result.session.sessionId,
      expiresAt: new Date(result.session.expiresAtMs).toISOString(),
      scopes: [...result.session.scopes] as Session['scopes'],
    }
    events.publish({ kind: 'session', expiresAt: session.expiresAt })
    return c.json(session)
  })

  // 4. Everything under /api from here on needs a bearer token.
  app.use('/api/*', async (c, next) => {
    const session = auth.authorize(c.req.header('authorization'))
    if (session === null) throw Problems.unauthenticated('a valid bearer token is required')
    c.set('session', session)
    await next()
  })

  const ensureScope = (session: SessionRecord, scope: string): void => {
    if (!auth.hasScope(session, scope)) throw Problems.forbidden(`this session lacks the ${scope} scope`)
  }

  const parseBody = async <T>(
    c: { req: { json: () => Promise<unknown> } },
    schema: { safeParse(input: unknown): { success: boolean; data?: T } },
  ): Promise<T> => {
    const raw = await c.req.json().catch(() => undefined)
    const parsed = schema.safeParse(raw)
    if (!parsed.success || parsed.data === undefined)
      throw Problems.invalid('request body does not satisfy the contract')
    return parsed.data
  }

  const parseIdParam = (value: string, what: string): string => {
    if (!ScanIdSchema.safeParse(value).success) throw Problems.invalid(`invalid ${what}`)
    return value
  }

  app.get('/api/v1/capabilities', (c) => {
    ensureScope(c.get('session'), 'scan:read')
    return c.json(capabilities)
  })

  /**
   * Preset scan targets: exactly the roots this host was configured with.
   * Capacity comes from one `df` call so the picker can show used/free
   * before any scan runs; roots `df` cannot report keep size 0 and no
   * `used`, which the UI renders as "unknown".
   */
  app.get('/api/v1/roots', async (c) => {
    ensureScope(c.get('session'), 'scan:read')
    const capacities = await volumeCapacities(config.roots)
    return c.json({
      roots: config.roots.map((rootPath, index) => {
        const capacity = capacities.get(rootPath)
        return {
          id: `root_${index}`,
          label: basename(rootPath) || rootPath,
          path: rootPath,
          kind: 'folder' as const,
          description: '',
          size: capacity?.totalBytes ?? 0,
          ...(capacity ? { used: capacity.usedBytes } : {}),
          source: 'preset' as const,
          removable: false,
        }
      }),
    })
  })

  /**
   * Mounted volumes with capacity, for the pre-scan "all my drives" list.
   * Display-only: mount points are facts, not scan targets — the picker
   * offers a scan only for paths inside the host's configured roots.
   */
  app.get('/api/v1/volumes', async (c) => {
    ensureScope(c.get('session'), 'scan:read')
    return c.json({ volumes: await mountedVolumes() })
  })

  app.post('/api/v1/scans', async (c) => {
    ensureScope(c.get('session'), 'scan:start')
    const request = await parseBody<Parameters<ScanManager['start']>[0]>(c, ScanStartRequestSchema)
    const session = await scans.start(request, request.label)
    return c.json(session, 201)
  })

  app.get('/api/v1/scans', (c) => {
    ensureScope(c.get('session'), 'scan:read')
    return c.json({ scans: scans.list() })
  })

  app.get('/api/v1/scans/:scanId', (c) => {
    ensureScope(c.get('session'), 'scan:read')
    return c.json(scans.status(parseIdParam(c.req.param('scanId'), 'scanId')))
  })

  app.post('/api/v1/scans/:scanId/cancel', (c) => {
    ensureScope(c.get('session'), 'scan:start')
    return c.json(scans.cancel(parseIdParam(c.req.param('scanId'), 'scanId')))
  })

  app.post('/api/v1/tree/slice', async (c) => {
    ensureScope(c.get('session'), 'scan:read')
    const request = await parseBody<{ scanId: string; nodeId: string; depth: number; maxChildrenPerNode: number }>(
      c,
      TreeSliceRequestSchema,
    )
    return c.json(scans.slice(request.scanId, request.nodeId, request.depth, request.maxChildrenPerNode))
  })

  app.post('/api/v1/tree/children', async (c) => {
    ensureScope(c.get('session'), 'scan:read')
    const request = await parseBody<{
      scanId: string
      nodeId: string
      offset: number
      limit: number
      sort: 'size' | 'name' | 'path'
    }>(c, ChildrenPageRequestSchema)
    return c.json(scans.children(request.scanId, request.nodeId, request.offset, request.limit, request.sort))
  })

  app.post('/api/v1/discovery', async (c) => {
    ensureScope(c.get('session'), 'scan:read')
    const request = await parseBody<DiscoveryRequest>(c, DiscoveryRequestSchema)
    return c.json(await scans.discover(request))
  })

  app.post('/api/v1/cleanup/plan', async (c) => {
    ensureScope(c.get('session'), 'cleanup:plan')
    const request = await parseBody<{ scanId: string; nodeIds: string[] }>(c, CleanupPlanRequestSchema)
    return c.json(scans.plan(request.scanId, request.nodeIds))
  })

  app.post('/api/v1/cleanup/execute', async (c) => {
    ensureScope(c.get('session'), 'cleanup:execute')
    const request = await parseBody<{ planId: string; confirm: true }>(c, CleanupExecuteRequestSchema)
    return c.json(await scans.execute(request.planId))
  })

  app.post('/api/v1/apps/audit', async (c) => {
    ensureScope(c.get('session'), 'scan:read')
    if (activeAppAudits >= MAX_CONCURRENT_APP_AUDITS) {
      throw Problems.limit('app audit is busy; try again shortly')
    }
    const request = await parseBody<AppAuditRequest>(c, AppAuditRequestSchema)
    // Same binary mechanism as protected scans: explicit config path or
    // SPACLENS_LOCAL_SCAN_BIN env / repository discovery. Unresolved reads
    // as the standard Unavailable problem.
    const binary = resolveLocalScanBin(localScanBin)
    activeAppAudits += 1
    try {
      let raw: unknown
      try {
        raw = await runAppAuditCli({ binary, request, homeOverride: config.appAuditHome })
      } catch (error) {
        throw Problems.internal(
          `app audit failed: ${error instanceof Error ? error.message.slice(0, 500) : String(error)}`,
        )
      }
      const parsed = AppAuditReportSchema.safeParse(raw)
      if (!parsed.success) throw Problems.internal('app audit returned an unexpected report shape')
      return c.json(parsed.data)
    } finally {
      activeAppAudits -= 1
    }
  })

  app.post('/api/v1/apps/icon', async (c) => {
    ensureScope(c.get('session'), 'scan:read')
    const body = await parseBody<{ appPath: string }>(c, AppIconRequestSchema)
    const binary = resolveLocalScanBin(localScanBin)
    try {
      return c.json(await runAppIconCli(binary, body.appPath))
    } catch (error) {
      // The binary-resolution failure above already threw Unavailable; only
      // CLI execution failures (missing/unreadable icon) land here.
      if (error instanceof ProblemError) throw error
      throw Problems.notFound('no icon available for this app')
    }
  })

  // 4c. Reveal a scanned path in the host's own file manager. The path is
  // data, never a command: the host resolves it against the roots it serves
  // and refuses anything outside them before a process is spawned.
  app.post('/api/v1/host/reveal', async (c) => {
    ensureScope(c.get('session'), 'host:reveal')
    const request = await parseBody<{ path: string; action?: RevealAction }>(c, RevealPathRequestSchema)
    const outcome = await revealPath(request.path, config.roots, {
      launch: deps.revealLauncher,
      action: request.action ?? 'reveal',
    })
    if (!outcome.ok) {
      if (outcome.reason === 'invalid') throw Problems.invalid(outcome.message)
      if (outcome.reason === 'forbidden') throw Problems.forbidden(outcome.message)
      if (outcome.reason === 'notFound') throw Problems.notFound(outcome.message)
      throw Problems.unavailable(outcome.message)
    }
    return c.json({ revealed: true })
  })

  // 5. AI advisor: only when the operator configured one. The base URL, model
  // and key live on the host; nothing here lets a client change them. When the
  // feature is off the routes answer the standard Unavailable problem (and no
  // session carries the scope in the first place).
  const requireAi = (): NonNullable<ResolvedServeConfig['ai']> => {
    if (config.ai === null) {
      throw Problems.unavailable(
        'this host has no AI advisor configured (start it with --ai-base-url and --ai-model)',
        false,
      )
    }
    return config.ai
  }

  app.get('/api/v1/ai/config', (c) => {
    const ai = requireAi()
    ensureScope(c.get('session'), 'ai:chat')
    const view: AiConfigView = {
      baseUrl: ai.baseUrl,
      model: ai.model,
      hasKey: ai.apiKey !== null,
      disableThinking: ai.disableThinking,
      managed: true,
    }
    return c.json(view)
  })

  app.post('/api/v1/ai/chat', async (c) => {
    const ai = requireAi()
    ensureScope(c.get('session'), 'ai:chat')
    const request = await parseBody<AiChatRequest>(c, AiChatRequestSchema)
    const forwarded = {
      endpoint: ai.endpoint,
      model: ai.model,
      apiKey: ai.apiKey,
      disableThinking: ai.disableThinking,
      messages: request.messages,
      tools: request.tools,
    }
    if (request.stream === true) {
      // Every failure before the first forwarded byte (bad request, timeout,
      // non-2xx upstream) is thrown here and answered with the normal problem
      // JSON; only a live stream reaches the non-JSON response below. A client
      // disconnect aborts the upstream request through this same signal.
      const stream = await forwardChatStream({ ...forwarded, signal: c.req.raw.signal })
      return new Response(stream, {
        status: 200,
        headers: {
          'content-type': 'text/event-stream',
          'cache-control': 'no-store',
          'x-accel-buffering': 'no',
        },
      })
    }
    return c.json(await forwardChatCompletion(forwarded))
  })

  // 5b. Memory view and process control: the host never re-implements a rule,
  // so every route forwards exactly one call to the `spacelens memory stdio`
  // child and re-validates its reply against the same contract schema the
  // client uses (a malformed reply is InternalError, never forwarded). The
  // bridge's presence is the feature switch: no bridge (started without
  // `--allow-memory`, or the release CLI could not be resolved) answers the
  // standard Unavailable problem, exactly like the AI routes without a config.
  const requireMemory = (): MemoryBridge => {
    if (memory === undefined) {
      throw Problems.unavailable(
        'this host does not have the memory view enabled (start it with --allow-memory)',
        false,
      )
    }
    return memory
  }

  /** Feature gate first, then scope, so a disabled feature never leaks scope state. */
  const memoryGate = (session: SessionRecord, scope: 'memory:read' | 'memory:terminate'): MemoryBridge => {
    const bridge = requireMemory()
    ensureScope(session, scope)
    return bridge
  }

  /** Re-validate one child reply; anything off-contract becomes InternalError. */
  const parseMemoryReply = <T>(
    raw: unknown,
    schema: { safeParse(input: unknown): { success: boolean; data?: T } },
  ): T => {
    const parsed = schema.safeParse(raw)
    if (!parsed.success || parsed.data === undefined) {
      throw Problems.internal('the memory process returned a reply that does not satisfy the contract')
    }
    return parsed.data
  }

  app.get('/api/v1/memory/snapshot', async (c) => {
    const session = c.get('session')
    const bridge = memoryGate(session, 'memory:read')
    return c.json(parseMemoryReply(await bridge.call(session.sessionId, { method: 'snapshot' }), MemorySnapshotSchema))
  })

  app.post('/api/v1/memory/history', async (c) => {
    const session = c.get('session')
    const bridge = memoryGate(session, 'memory:read')
    const request = await parseBody<HistoryRequest>(c, HistoryRequestSchema)
    const call: MemoryCall = { method: 'history', request }
    return c.json(parseMemoryReply(await bridge.call(session.sessionId, call), MemoryHistorySchema))
  })

  app.post('/api/v1/memory/candidates', async (c) => {
    const session = c.get('session')
    const bridge = memoryGate(session, 'memory:read')
    const request = await parseBody<CandidatesRequest>(c, CandidatesRequestSchema)
    const call: MemoryCall = { method: 'candidates', request }
    return c.json(parseMemoryReply(await bridge.call(session.sessionId, call), FreeUpCandidatesSchema))
  })

  app.get('/api/v1/memory/audit', async (c) => {
    const session = c.get('session')
    const bridge = memoryGate(session, 'memory:read')
    return c.json(parseMemoryReply(await bridge.call(session.sessionId, { method: 'audit' }), AuditLogSchema))
  })

  app.post('/api/v1/memory/terminate/plan', async (c) => {
    const session = c.get('session')
    const bridge = memoryGate(session, 'memory:terminate')
    const request = await parseBody<TerminatePlanRequest>(c, TerminatePlanRequestSchema)
    const call: MemoryCall = { method: 'plan', request }
    return c.json(parseMemoryReply(await bridge.call(session.sessionId, call), TerminatePlanSchema))
  })

  app.post('/api/v1/memory/terminate/execute', async (c) => {
    const session = c.get('session')
    const bridge = memoryGate(session, 'memory:terminate')
    const request = await parseBody<TerminateExecuteRequest>(c, TerminateExecuteRequestSchema)
    const call: MemoryCall = { method: 'execute', request }
    return c.json(parseMemoryReply(await bridge.call(session.sessionId, call), TerminateOutcomeSchema))
  })

  // 6. SSE: fetch-based clients (EventSource cannot send Authorization).
  app.get('/api/v1/events', (c) => {
    ensureScope(c.get('session'), 'scan:read')
    const since = Number.parseInt(c.req.query('since') ?? '0', 10)
    if (!Number.isInteger(since) || since < 0) throw Problems.invalid('since must be a non-negative integer')

    const replayed = events.replay(since)
    const encoder = new TextEncoder()
    let unsubscribe: (() => void) | null = null
    let heartbeat: ReturnType<typeof setInterval> | null = null

    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        const push = (text: string) => controller.enqueue(encoder.encode(text))
        const cleanup = () => {
          unsubscribe?.()
          unsubscribe = null
          if (heartbeat !== null) clearInterval(heartbeat)
          heartbeat = null
        }
        push(frame('hello', 0, { highWatermark: events.highWatermark }))
        for (const envelope of replayed.events) push(frame(envelope.payload.kind, envelope.sequence, envelope))
        if (replayed.gap !== null) {
          push(
            frame('eventGap', replayed.gap.toSequence, {
              sequence: replayed.gap.toSequence,
              emittedAt: new Date().toISOString(),
              payload: {
                kind: 'eventGap',
                fromSequence: replayed.gap.fromSequence,
                toSequence: replayed.gap.toSequence,
              },
            }),
          )
        }
        unsubscribe = events.subscribe((envelope) => push(frame(envelope.payload.kind, envelope.sequence, envelope)))
        heartbeat = setInterval(() => push(': keep-alive\n\n'), 15_000)
        c.req.raw.signal.addEventListener('abort', () => {
          cleanup()
          try {
            controller.close()
          } catch {
            // already closed
          }
        })
      },
      cancel() {
        unsubscribe?.()
        if (heartbeat !== null) clearInterval(heartbeat)
      },
    })
    return new Response(stream, {
      status: 200,
      headers: {
        'content-type': 'text/event-stream; charset=utf-8',
        'cache-control': 'no-store',
        'x-accel-buffering': 'no',
      },
    })
  })

  // 7. Unknown API paths are a JSON problem, never the SPA.
  const unknownApi = (c: { req: { url: string } }): never => {
    throw Problems.notFound(`no such API route: ${new URL(c.req.url).pathname}`)
  }
  app.get('/api/*', unknownApi)
  app.post('/api/*', unknownApi)

  // 8. Preflight for allowed cross-origin UIs, then the SPA fallback.
  app.options('*', (c) => {
    const origin = c.req.header('origin') ?? null
    if (origin === null || !config.uiOrigins.includes(origin)) throw Problems.forbidden('origin not allowed')
    return c.body(null, 204)
  })

  app.all('*', async (c) => {
    const pathname = new URL(c.req.url).pathname
    if (c.req.method !== 'GET' && c.req.method !== 'HEAD')
      throw Problems.invalid('only GET is served for non-API paths')
    return assets(pathname)
  })

  app.onError((error, c) => {
    const cors = corsHeadersFor(c.req.header('origin') ?? null)
    if (error instanceof ProblemError) {
      return jsonResponse({ problem: error.problem }, error.status, cors)
    }
    const correlation = Math.random().toString(16).slice(2)
    return jsonResponse(
      { problem: { code: 'InternalError', message: 'internal error', retryable: false, details: { correlation } } },
      500,
      { 'x-spacelens-correlation': correlation, ...cors },
    )
  })

  return app
}
