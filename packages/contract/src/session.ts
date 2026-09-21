import { z } from 'zod'

export const SCOPES = ['scan:read', 'scan:start', 'cleanup:plan', 'cleanup:execute'] as const

export type Scope = (typeof SCOPES)[number]

export const ScopeSchema = z.enum(SCOPES)

export const SessionExchangeRequestSchema = z.strictObject({
  ticket: z.string().min(16).max(512),
  /** Required only when the host was started with a hosted password. */
  password: z.string().min(1).max(256).optional(),
})

export type SessionExchangeRequest = z.infer<typeof SessionExchangeRequestSchema>

export const SessionSchema = z.strictObject({
  token: z.string().min(16).max(256),
  sessionId: z.string().min(1).max(128),
  expiresAt: z.iso.datetime(),
  scopes: z.array(ScopeSchema),
})

export type Session = z.infer<typeof SessionSchema>
