import { networkInterfaces } from 'node:os'
import { isIP } from 'node:net'
import { validateCidrs } from './cidr.ts'
import type { TrashPort } from './trash.ts'

export const DEFAULT_PORT = 9420
export const HOSTED_PASSWORD_ENV = 'SPACLENS_HOSTED_PASSWORD'

export class ConfigError extends Error {}

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
  }
}
