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
 * The loopback spellings of one service's origin.
 *
 * `http://127.0.0.1:PORT`, `http://localhost:PORT` and `http://[::1]:PORT` are
 * the same machine, the same listener and the same trust domain — the origin
 * policy already treats them as one service's authorities because a user may
 * open either spelling. A pairing ticket minted while the document was served
 * on one spelling must therefore redeem from any of them; anything else turns
 * "which spelling is in the address bar" into a pairing failure.
 *
 * Non-loopback origins never match here: a hosted ticket stays bound to the
 * exact site it was minted for.
 */
function sameLoopbackService(first: string, second: string): boolean {
  if (first === second) {
    return true
  }
  const loopback = (origin: string): string | null => {
    const match = /^http:\/\/(127\.0\.0\.1|localhost|\[::1\]):(\d+)$/.exec(origin)
    if (match === null || match[2] === undefined) {
      return null
    }
    return match[2]
  }
  const firstPort = loopback(first)
  if (firstPort === null) {
    return false
  }
  return loopback(second) === firstPort
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
    if (record.expiresAtMs <= Date.now()) {
      // Expired tickets are spent: retrying an expired ticket must not read as
      // "already used".
      this.tickets.delete(key)
      return { ok: false, reason: 'expired' }
    }
    if (record.serviceInstanceId !== this.options.serviceInstanceId) {
      this.tickets.delete(key)
      return { ok: false, reason: 'unknown' }
    }
    if (!sameLoopbackService(record.origin, input.origin ?? '')) {
      // A refused origin does not consume the ticket: the holder never spent
      // it, and a single probe from another allowed origin must not turn the
      // legitimate page's next attempt into an "already used" failure. A
      // successful exchange still consumes it exactly once, below.
      return { ok: false, reason: 'origin-mismatch' }
    }
    if (record.passwordRequired) {
      // A hosted password failure is retryable with the same ticket, but the
      // request-rate limiter around the endpoint bounds guessing. Invalid
      // instance and expiry failures above still consume the ticket on first
      // sight; a refused origin does not, so the holder can still redeem it.
      if (!this.verifyPassword(input.password)) return { ok: false, reason: 'password' }
    }
    // A successful exchange consumes the ticket exactly once, here.
    this.tickets.delete(key)
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
