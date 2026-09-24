import { Hono } from 'hono'
import { basename } from 'node:path'

import type { Capabilities, Health, Session } from '@space-lens/contract'
import {
  ChildrenPageRequestSchema,
  CleanupExecuteRequestSchema,
  CleanupPlanRequestSchema,
  ScanIdSchema,
  ScanStartRequestSchema,
  SessionExchangeRequestSchema,
  TreeSliceRequestSchema,
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

export interface HostDeps {
  config: ResolvedServeConfig
  auth: AuthService
  scans: ScanManager
  events: EventRing
  assets: AssetHandler
  health: Health
  capabilities: Capabilities
  allowlist: ClientAllowlist
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
  const { config, auth, scans, events, assets, health, capabilities, allowlist } = deps

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
      { hostHeader: c.req.header('host') ?? null, origin, secFetchSite: c.req.header('sec-fetch-site') ?? null },
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

  /** Preset scan targets: exactly the roots this host was configured with. */
  app.get('/api/v1/roots', (c) => {
    ensureScope(c.get('session'), 'scan:read')
    return c.json({
      roots: config.roots.map((rootPath, index) => ({
        id: `root_${index}`,
        label: basename(rootPath) || rootPath,
        path: rootPath,
        kind: 'folder' as const,
        description: '',
        size: 0,
        source: 'preset' as const,
        removable: false,
      })),
    })
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

  // 5. SSE: fetch-based clients (EventSource cannot send Authorization).
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

  // 6. Unknown API paths are a JSON problem, never the SPA.
  const unknownApi = (c: { req: { url: string } }): never => {
    throw Problems.notFound(`no such API route: ${new URL(c.req.url).pathname}`)
  }
  app.get('/api/*', unknownApi)
  app.post('/api/*', unknownApi)

  // 7. Preflight for allowed cross-origin UIs, then the SPA fallback.
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
