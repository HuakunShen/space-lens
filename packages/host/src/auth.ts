import { createHash, randomBytes, scryptSync, timingSafeEqual } from 'node:crypto'

function sha256(value: string): string {
  return createHash('sha256').update(value).digest('hex')
}

function randomId(prefix: string, bytes = 32): string {
  return `${prefix}${randomBytes(bytes).toString('base64url')}`
}

const MAX_TICKETS = 64
const MAX_SESSIONS = 16

export interface TicketRecord {
  origin: string
  passwordRequired: boolean
  expiresAtMs: number
  serviceInstanceId: string
}

export interface SessionRecord {
  token: string
  sessionId: string
  serviceInstanceId: string
  scopes: readonly string[]
  expiresAtMs: number
}

export type ExchangeFailure = 'unknown' | 'expired' | 'origin-mismatch' | 'password'

export type ExchangeResult = { ok: true; session: SessionRecord } | { ok: false; reason: ExchangeFailure }

export interface AuthServiceOptions {
  serviceInstanceId: string
  ticketTtlMs: number
  sessionTtlMs: number
  /** scrypt-hashed hosted password, or null when pairing needs no password. */
  hostedPassword?: { salt: Buffer; key: Buffer } | null
  scopes: readonly string[]
}

export function hashHostedPassword(password: string): { salt: Buffer; key: Buffer } {
  const salt = randomBytes(16)
  return { salt, key: scryptSync(password, salt, 32, { N: 16384 }) }
}

/**
 * In-memory ticket and session store. Credentials never touch disk: a restart
 * invalidates everything, which is the correct failure mode for a host whose
 * only durable secret is the terminal it was started from.
 */
export class AuthService {
  private tickets = new Map<string, TicketRecord>()
  private sessions = new Map<string, SessionRecord>()
  private readonly scopes: readonly string[]
  private readonly options: AuthServiceOptions

  constructor(options: AuthServiceOptions) {
    this.options = options
    this.scopes = options.scopes
  }

  mintTicket(origin: string, passwordRequired: boolean): string {
    this.sweep()
    if (this.tickets.size >= MAX_TICKETS) {
      const oldest = this.tickets.keys().next().value
      if (oldest !== undefined) this.tickets.delete(oldest)
    }
    const ticket = randomId('', 32)
    this.tickets.set(sha256(ticket), {
      origin,
      passwordRequired,
      expiresAtMs: Date.now() + this.options.ticketTtlMs,
      serviceInstanceId: this.options.serviceInstanceId,
    })
    return ticket
  }

  /**
   * A machine ticket binds to the empty origin: the supervisor spending it
   * sends no Origin header at all, and no browser can ever spend it.
   */
  exchange(ticket: string, input: { origin: string | null; password?: string }): ExchangeResult {
    const key = sha256(ticket)
    const record = this.tickets.get(key)
    if (record === undefined) return { ok: false, reason: 'unknown' }
    // single use: consumed on first sight, whatever the verdict below
    this.tickets.delete(key)
    if (record.expiresAtMs <= Date.now()) return { ok: false, reason: 'expired' }
    if (record.serviceInstanceId !== this.options.serviceInstanceId) return { ok: false, reason: 'unknown' }
    if (record.origin !== (input.origin ?? '')) return { ok: false, reason: 'origin-mismatch' }
    if (record.passwordRequired) {
      if (!this.verifyPassword(input.password)) return { ok: false, reason: 'password' }
    }
    const session: SessionRecord = {
      token: randomId('sls_'),
      sessionId: randomId('sess_', 12),
      serviceInstanceId: this.options.serviceInstanceId,
      scopes: [...this.scopes],
      expiresAtMs: Date.now() + this.options.sessionTtlMs,
    }
    if (this.sessions.size >= MAX_SESSIONS) {
      const oldest = this.sessions.keys().next().value
      if (oldest !== undefined) this.sessions.delete(oldest)
    }
    this.sessions.set(sha256(session.token), session)
    return { ok: true, session }
  }

  authorize(bearer: string | null | undefined): SessionRecord | null {
    if (!bearer?.startsWith('Bearer ')) return null
    const token = bearer.slice('Bearer '.length)
    const record = this.sessions.get(sha256(token))
    if (record === undefined) return null
    if (record.expiresAtMs <= Date.now()) {
      this.sessions.delete(sha256(token))
      return null
    }
    return record
  }

  expireSession(token: string): void {
    this.sessions.delete(sha256(token))
  }

  hasScope(record: SessionRecord, scope: string): boolean {
    return record.scopes.includes(scope)
  }

  sessionExpiresAt(): number | null {
    let latest: number | null = null
    for (const session of this.sessions.values()) {
      if (latest === null || session.expiresAtMs > latest) latest = session.expiresAtMs
    }
    return latest
  }

  private verifyPassword(password: string | undefined): boolean {
    const hosted = this.options.hostedPassword
    if (hosted === null || hosted === undefined) return false
    if (password === undefined || password.length < 12) return false
    const candidate = scryptSync(password, hosted.salt, 32, { N: 16384 })
    return timingSafeEqual(candidate, hosted.key)
  }

  private sweep(): void {
    const now = Date.now()
    for (const [key, record] of this.tickets) if (record.expiresAtMs <= now) this.tickets.delete(key)
    for (const [key, record] of this.sessions) if (record.expiresAtMs <= now) this.sessions.delete(key)
  }
}
