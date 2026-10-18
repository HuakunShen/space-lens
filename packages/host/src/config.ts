import { networkInterfaces } from 'node:os'
import { isIP } from 'node:net'
import { chatEndpoint } from './ai.ts'
import { validateCidrs } from './cidr.ts'
import type { TrashPort } from './trash.ts'

export const DEFAULT_PORT = 9420
export const HOSTED_PASSWORD_ENV = 'SPACLENS_HOSTED_PASSWORD'
/** Host-only AI API key. Never a flag, never returned to a client, never printed. */
export const AI_API_KEY_ENV = 'SPACLENS_AI_API_KEY'

export class ConfigError extends Error {}

/**
 * Operator-owned AI advisor configuration. Its presence enables the
 * `/api/v1/ai/*` routes and the `ai:chat` scope. There is deliberately no
 * client-facing route that changes any field: a client-settable base URL would
 * be an SSRF and key-theft surface.
 */
export interface ServeAiOptions {
  baseUrl: string
  model: string
  /** Falls back to {@link AI_API_KEY_ENV} when omitted. */
  apiKey?: string | null
  disableThinking?: boolean
}

/** Validated, resolved AI advisor config. `null` means the feature is off. */
export interface ResolvedAiConfig {
  /** Validated chat-completions endpoint (`…/chat/completions`). */
  endpoint: string
  /** Operator-provided base URL, trimmed (what the config view reports). */
  baseUrl: string
  model: string
  /** The key, or null when none is configured. Never returned to a client. */
  apiKey: string | null
  disableThinking: boolean
}

export interface ServeOptions {
  /**
   * Bind address: `loopback` (default), `0.0.0.0` / `::` for all interfaces,
   * an interface name such as `en0`, or an explicit IP address.
   */
  host?: string
  port?: number
  /** Client address allowlist (CIDR). Required when binding a non-loopback address. */
  allowCidr?: readonly string[]
  /** Explicit acceptance of any client when binding a non-loopback address. */
  allowLan?: boolean
  /** Grant cleanup scopes to paired sessions. Without it the server is read-only. */
  allowCleanup?: boolean
  /**
   * Enable the read-only memory view: the `/api/v1/memory/*` read routes and
   * the `memory:read` scope. Off unless explicitly requested.
   */
  allowMemory?: boolean
  /**
   * Allow process control (terminate plan/execute) from paired sessions. It
   * requires `allowMemory`, a loopback bind and loopback-only UI origins;
   * `resolveServeConfig` refuses anything else with a {@link ConfigError}.
   */
  allowProcessControl?: boolean
  /** Exact origins allowed to talk to this API cross-origin (Cloudflare PWA). */
  uiOrigins?: readonly string[]
  ticketTtlSeconds?: number
  sessionTtlSeconds?: number
  /** Hosted password from the environment. Required when a non-loopback UI origin is configured. */
  hostedPassword?: string | null
  /** Directories scans may target. Defaults to the process working directory. */
  roots?: readonly string[]
  webRoot?: string | null
  open?: boolean
  json?: boolean
  /** Extension mode: loopback, port 0, no origin, readiness JSON on stdout. */
  machine?: boolean
  trustProxy?: boolean
  maxConcurrentScans?: number
  maxTotalScans?: number
  /** Trash backend override (embedders and tests); defaults to the OS trash. */
  trash?: TrashPort
  /** Absolute protected release scanner executable; defaults to env/repository lookup. */
  localScanBin?: string
  /**
   * Home directory override for the app-storage audit CLI spawn. When set,
   * the host passes `--home` to `spacelens app-audit`; otherwise the CLI
   * inherits the host process environment (no flag is passed).
   */
  appAuditHome?: string
  /**
   * `frame-ancestors` sources for served documents. Defaults to `['none']` —
   * the UI frames nobody. An embedder that mounts this host behind its own
   * route passes `['self']` so its same-origin page may frame the workbench.
   */
  frameAncestors?: readonly string[]
  /**
   * Operator-owned AI advisor. Presence enables `/api/v1/ai/*` and grants the
   * `ai:chat` scope to paired sessions; the API key comes from `apiKey` or
   * {@link AI_API_KEY_ENV}, never from a client and never from a flag.
   */
  ai?: ServeAiOptions
}

export interface ResolvedHost {
  address: string
  loopback: boolean
  display: string
}

export interface ResolvedServeConfig {
  bind: ResolvedHost
  port: number
  /** True when --port was given explicitly; a busy explicit port is refused, a busy default falls back. */
  portExplicit: boolean
  allowCidr: string[]
  allowLan: boolean
  allowCleanup: boolean
  /** Read-only memory view (`memory:read`, `/api/v1/memory/*`). */
  allowMemory: boolean
  /** Process control (`memory:terminate`); only ever true with a loopback bind. */
  allowProcessControl: boolean
  uiOrigins: string[]
  ticketTtlMs: number
  sessionTtlMs: number
  hostedPassword: string | null
  roots: string[]
  webRoot: string | null
  open: boolean
  json: boolean
  machine: boolean
  trustProxy: boolean
  maxConcurrentScans: number
  maxTotalScans: number
  frameAncestors: string[]
  /** Home override for `spacelens app-audit --home`; null inherits the process env. */
  appAuditHome: string | null
  /** Operator AI advisor config, or null when the feature is off. */
  ai: ResolvedAiConfig | null
}

export function resolveBindHost(input: string | undefined): ResolvedHost {
  const raw = (input ?? 'loopback').trim().toLowerCase()
  if (['loopback', 'localhost', '127.0.0.1'].includes(raw)) {
    return { address: '127.0.0.1', loopback: true, display: 'localhost' }
  }
  if (raw === '::1') return { address: '::1', loopback: true, display: '[::1]' }
  if (['0.0.0.0', '*', 'any', 'all'].includes(raw)) {
    return { address: '0.0.0.0', loopback: false, display: '0.0.0.0' }
  }
  if (raw === '::') return { address: '::', loopback: false, display: '[::]' }

  const interfaces = networkInterfaces()
  if (Object.hasOwn(interfaces, raw)) {
    const addresses = interfaces[raw] ?? []
    for (const info of addresses) {
      if (info.family === 'IPv4' && !info.internal) {
        return { address: info.address, loopback: false, display: `${raw} (${info.address})` }
      }
    }
    for (const info of addresses) {
      if (!info.internal) return { address: info.address, loopback: false, display: `${raw} (${info.address})` }
    }
  }

  if (isIP(raw) !== 0) {
    const loopback = raw.startsWith('127.') || raw === '::1'
    return { address: raw, loopback, display: isIP(raw) === 6 ? `[${raw}]` : raw }
  }
  throw new ConfigError(`unknown --host value: ${input}`)
}

const ORIGIN_PATTERN = /^https?:\/\/[a-z0-9.-]+(:\d{1,5})?$/

export function normalizeOrigin(origin: string, label = '--ui-origin'): string {
  const normalized = origin.trim().toLowerCase().replace(/\/$/, '')
  if (!ORIGIN_PATTERN.test(normalized)) {
    throw new ConfigError(`${label} must be an exact http(s) origin without path or credentials: ${origin}`)
  }
  return normalized
}

export function isLoopbackOrigin(origin: string): boolean {
  try {
    const url = new URL(origin)
    const host = url.hostname
    return host === '127.0.0.1' || host === 'localhost' || host === '::1' || host === '[::1]'
  } catch {
    return false
  }
}

/**
 * Validates and resolves the operator AI advisor config. A malformed base URL
 * is a startup failure (`ConfigError`), not a request-time surprise: the same
 * guard the request path uses is applied here.
 *
 * The key is tri-state: `apiKey` string wins, `apiKey: null` means explicitly
 * none, and an omitted/empty `apiKey` falls back to {@link AI_API_KEY_ENV}.
 */
export function resolveAiConfig(options: ServeAiOptions | undefined): ResolvedAiConfig | null {
  if (options === undefined) return null
  const baseUrl = options.baseUrl.trim()
  let endpoint: string
  try {
    endpoint = chatEndpoint(baseUrl)
  } catch (error) {
    throw new ConfigError(error instanceof Error ? error.message : String(error))
  }
  const model = options.model.trim()
  if (model === '') throw new ConfigError('the AI model must not be empty')
  const envKey = process.env[AI_API_KEY_ENV]?.trim() ?? ''
  const optionKey = options.apiKey?.trim() ?? ''
  let apiKey: string | null
  if (options.apiKey === null) apiKey = null
  else if (optionKey !== '') apiKey = optionKey
  else apiKey = envKey !== '' ? envKey : null
  return { endpoint, baseUrl, model, apiKey, disableThinking: options.disableThinking ?? false }
}

function parsePort(port: number | undefined): number {
  if (port === undefined) return DEFAULT_PORT
  if (!Number.isInteger(port) || port < 0 || port > 65535) {
    throw new ConfigError(`--port must be an integer between 0 and 65535: ${port}`)
  }
  return port
}

export function resolveServeConfig(options: ServeOptions): ResolvedServeConfig {
  const machine = options.machine ?? false

  let bind = resolveBindHost(options.host)
  let port = parsePort(options.port)
  let json = options.json ?? false
  let open = options.open ?? false
  let uiOrigins = (options.uiOrigins ?? []).map((origin) => normalizeOrigin(origin))
  let allowLan = options.allowLan ?? false
  let allowCidr = [...(options.allowCidr ?? [])]

  if (machine) {
    bind = { address: '127.0.0.1', loopback: true, display: 'localhost' }
    port = 0
    json = true
    open = false
    allowLan = false
    allowCidr = []
    uiOrigins = []
  }

  validateCidrs(allowCidr)

  if (!bind.loopback && allowCidr.length === 0 && !allowLan) {
    throw new ConfigError(
      `binding ${bind.display} exposes the server beyond this machine; ` +
        'pass --allow-cidr <cidr> (repeatable) to name the allowed networks, or --allow-lan to accept any client explicitly',
    )
  }

  const optionPassword = typeof options.hostedPassword === 'string' ? options.hostedPassword.trim() : null
  const hostedPassword = optionPassword ?? (process.env[HOSTED_PASSWORD_ENV]?.trim() || null)
  if (uiOrigins.some((origin) => !isLoopbackOrigin(origin)) && hostedPassword === null) {
    throw new ConfigError(
      `a non-loopback --ui-origin requires ${HOSTED_PASSWORD_ENV} in the environment; ` +
        'a ticket alone must never unlock a hosted UI',
    )
  }

  // Process control is the only remote-kill surface in the product, so it is
  // refused unless the whole host is local: a read-only memory view must be on
  // (otherwise there is nothing to stage a plan against), the listener must be
  // loopback, and every cross-origin UI must itself be loopback.
  const allowMemory = options.allowMemory ?? false
  const allowProcessControl = options.allowProcessControl ?? false
  if (allowProcessControl && !allowMemory) {
    throw new ConfigError('--allow-process-control requires --allow-memory (there is no view to act from otherwise)')
  }
  if (allowProcessControl && (!bind.loopback || uiOrigins.some((origin) => !isLoopbackOrigin(origin)))) {
    throw new ConfigError(
      '--allow-process-control requires a loopback bind and loopback-only --ui-origin values; ' +
        'quitting processes is never exposed to other machines',
    )
  }

  const roots = [...(options.roots ?? [process.cwd()])].map((root) =>
    root.startsWith('/') ? root : `${process.cwd()}/${root}`,
  )

  return {
    bind,
    port,
    portExplicit: options.port !== undefined,
    allowCidr,
    allowLan,
    allowCleanup: options.allowCleanup ?? false,
    allowMemory,
    allowProcessControl,
    uiOrigins,
    ticketTtlMs: (options.ticketTtlSeconds ?? 60) * 1000,
    sessionTtlMs: (options.sessionTtlSeconds ?? 8 * 60 * 60) * 1000,
    hostedPassword,
    roots,
    webRoot: options.webRoot ?? null,
    open,
    json,
    machine,
    trustProxy: options.trustProxy ?? false,
    maxConcurrentScans: options.maxConcurrentScans ?? 4,
    maxTotalScans: options.maxTotalScans ?? 8,
    frameAncestors: [...(options.frameAncestors ?? ["'none'"])],
    appAuditHome: options.appAuditHome ?? null,
    ai: resolveAiConfig(options.ai),
  }
}
