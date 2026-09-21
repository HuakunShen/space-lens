import { z } from 'zod'
import { CLEANUP_MODES } from './cleanup.ts'
import { API_MAJOR, CONTRACT_VERSION } from './version.ts'

const UintSchema = z.number().int().nonnegative()

export const CapabilitiesSchema = z.strictObject({
  apiMajor: z.literal(API_MAJOR),
  contractVersion: z.string(),
  scan: z.strictObject({
    start: z.boolean(),
    cancel: z.boolean(),
    maxConcurrent: UintSchema,
  }),
  cleanup: z.strictObject({
    plan: z.boolean(),
    execute: z.boolean(),
    mode: z.enum(CLEANUP_MODES),
  }),
  host: z.strictObject({
    /** Native folder pickers exist only in desktop/webview hosts, never over HTTP. */
    folderPicker: z.boolean(),
  }),
  icloud: z.enum(['unavailable']),
})

export type Capabilities = z.infer<typeof CapabilitiesSchema>

export const HealthSchema = z.strictObject({
  status: z.literal('ok'),
  serviceInstanceId: z.string().min(1),
  apiMajor: z.literal(API_MAJOR),
  contractVersion: z.string(),
})

export type Health = z.infer<typeof HealthSchema>

/**
 * Deliberately sparse: `/health` is the one unauthenticated API route (it
 * carries no user data), so it must not leak paths, labels, or scan state.
 */
export function healthPayload(serviceInstanceId: string): Health {
  return { status: 'ok', serviceInstanceId, apiMajor: API_MAJOR, contractVersion: CONTRACT_VERSION }
}
