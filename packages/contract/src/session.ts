import { z } from 'zod'

export const SCOPES = [
  'scan:read',
  'scan:start',
  'cleanup:plan',
  'cleanup:execute',
  'ai:chat',
  'memory:read',
  'memory:terminate',
  /** Open a scanned path in the host's own file manager. */
  'host:reveal',
] as const

export type Scope = (typeof SCOPES)[number]

export const ScopeSchema = z.enum(SCOPES)

export const SessionExchangeRequestSchema = z.strictObject({
  ticket: z.string().min(16).max(512),
  /** Required only when the host was started with a hosted password. */
  password: z.string().min(1).max(256).optional(),
})

export type SessionExchangeRequest = z.infer<typeof SessionExchangeRequestSchema>

/**
 * A paired supervisor (the CLI behind an SSH tunnel) asks the host to mint a
 * ticket a local browser can spend. The origin must be plain loopback HTTP —
 * the tunnel's local port — because that is the only place the minted ticket
 * is meant to be spent.
 */
export const BrowserTicketRequestSchema = z.strictObject({
  origin: z
    .string()
    .regex(/^http:\/\/(127\.0\.0\.1|localhost):[1-9][0-9]{0,4}$/),
})

export type BrowserTicketRequest = z.infer<typeof BrowserTicketRequestSchema>

export const BrowserTicketSchema = z.strictObject({
  ticket: z.string().min(16).max(512),
  /** Ready to open: the origin the ticket was minted for, with `?pair=`. */
  url: z.string().min(1),
})

export type BrowserTicket = z.infer<typeof BrowserTicketSchema>

export const SessionSchema = z.strictObject({
  token: z.string().min(16).max(256),
  sessionId: z.string().min(1).max(128),
  expiresAt: z.iso.datetime(),
  scopes: z.array(ScopeSchema),
})

export type Session = z.infer<typeof SessionSchema>
