/**
 * Security helpers for the standalone Space Lens browser host.
 * The local HTTP/WebSocket server is a privileged filesystem surface, so every
 * transport path must authenticate and check capabilities before touching scan
 * or cleanup APIs.
 */
import { randomBytes, timingSafeEqual } from 'node:crypto'

export type WebCapability =
  | 'scan:start'
  | 'scan:read'
  | 'scan:cancel'
  | 'cleanup:plan'
  | 'cleanup:execute'
  | 'host:reveal'

export interface WebSecurityContext {
  origin: string
  readonly bootToken: string
  readonly rpcToken: string
  readonly capabilities: ReadonlySet<WebCapability>
}

export interface WebSecurityOptions {
  readonly origin: string
  readonly bootToken?: string
  readonly rpcToken?: string
  readonly capabilities?: readonly WebCapability[]
  readonly allowCleanupExecute?: boolean
}

const DEFAULT_CAPABILITIES: readonly WebCapability[] = [
  'scan:start',
  'scan:read',
  'scan:cancel',
  'cleanup:plan',
  'host:reveal',
]

export function createWebSecurityContext(options: WebSecurityOptions): WebSecurityContext {
  const capabilities = new Set<WebCapability>(options.capabilities ?? DEFAULT_CAPABILITIES)
  if (options.allowCleanupExecute) {
    capabilities.add('cleanup:execute')
  }
  return {
    origin: options.origin.replace(/\/$/, ''),
    bootToken: options.bootToken ?? createToken(),
    rpcToken: options.rpcToken ?? createToken(),
    capabilities,
  }
}

export function createToken(): string {
  return randomBytes(32).toString('base64url')
}

export function hasCapability(security: WebSecurityContext, capability: WebCapability): boolean {
  return security.capabilities.has(capability)
}

export function assertCapability(security: WebSecurityContext, capability: WebCapability): void {
  if (!hasCapability(security, capability)) {
    throw new WebSecurityError(`Capability denied: ${capability}`, 403)
  }
}

export function authorizeBearer(
  security: WebSecurityContext,
  authorization: string | undefined,
  capability: WebCapability,
): WebSecurityError | null {
  const token = authorization?.startsWith('Bearer ') ? authorization.slice('Bearer '.length) : ''
  if (!verifyToken(security.rpcToken, token)) {
    return new WebSecurityError('Unauthorized', 401)
  }
  if (!hasCapability(security, capability)) {
    return new WebSecurityError(`Capability denied: ${capability}`, 403)
  }
  return null
}

export function authorizeBootToken(security: WebSecurityContext, token: string | null | undefined): boolean {
  return verifyToken(security.bootToken, token ?? '')
}

export function authorizeRpcUpgrade(
  security: WebSecurityContext,
  input: {
    readonly token: string | null | undefined
    readonly host: string | null | undefined
    readonly origin: string | null | undefined
  },
): WebSecurityError | null {
  if (!verifyToken(security.rpcToken, input.token ?? '')) {
    return new WebSecurityError('Unauthorized', 401)
  }
  if (!isAllowedHost(security, input.host)) {
    return new WebSecurityError('Forbidden', 403)
  }
  if (!isAllowedOrigin(security, input.origin, input.host)) {
    return new WebSecurityError('Forbidden', 403)
  }
  return null
}

export function rpcUrlFor(security: WebSecurityContext): string {
  const url = new URL(security.origin)
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
  url.pathname = '/rpc'
  url.search = ''
  url.searchParams.set('token', security.rpcToken)
  url.hash = ''
  return url.toString()
}

export function bootUrlFor(security: WebSecurityContext): string {
  const url = new URL(security.origin)
  url.pathname = '/space-lens.boot.json'
  url.search = ''
  url.searchParams.set('token', security.bootToken)
  url.hash = ''
  return url.toString()
}

export class WebSecurityError extends Error {
  constructor(
    message: string,
    readonly status: 401 | 403,
  ) {
    super(message)
  }
}

function verifyToken(expected: string, actual: string): boolean {
  if (!expected || !actual) return false
  const expectedBuffer = Buffer.from(expected)
  const actualBuffer = Buffer.from(actual)
  return expectedBuffer.byteLength === actualBuffer.byteLength && timingSafeEqual(expectedBuffer, actualBuffer)
}

function isAllowedHost(security: WebSecurityContext, host: string | null | undefined): boolean {
  if (!host) return false
  return host === new URL(security.origin).host
}

function isAllowedOrigin(
  security: WebSecurityContext,
  origin: string | null | undefined,
  host: string | null | undefined,
): boolean {
  if (!origin) return true
  if (!host) return false
  try {
    const parsed = new URL(origin)
    return parsed.origin === security.origin && parsed.host === host
  } catch {
    return false
  }
}
