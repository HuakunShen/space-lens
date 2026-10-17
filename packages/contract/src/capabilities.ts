import { z } from 'zod'
import { CLEANUP_MODES } from './cleanup.ts'
import { MEMORY_PLATFORMS } from './memory.ts'
import { API_MAJOR, CONTRACT_VERSION } from './version.ts'

const UintSchema = z.number().int().nonnegative()

export const CapabilitiesSchema = z.strictObject({
  apiMajor: z.literal(API_MAJOR),
  contractVersion: z.string(),
  scan: z.strictObject({
    start: z.boolean(),
    cancel: z.boolean(),
    discovery: z.boolean().optional(),
    appAudit: z.boolean().optional(),
    appScan: z.boolean().optional(),
    maxConcurrent: UintSchema,
  }),
  cleanup: z.strictObject({
    plan: z.boolean(),
    execute: z.boolean(),
    mode: z.enum(CLEANUP_MODES),
    /** V2 app-scoped cleanup plan/execute submit arms (audit-generation
     * authorized, outside the engine). Absent on older hosts. */
    appPlan: z.boolean().optional(),
    appExecute: z.boolean().optional(),
  }),
  host: z.strictObject({
    /** Native folder pickers exist only in desktop/webview hosts, never over HTTP. */
    folderPicker: z.boolean(),
    /** Finder reveal of an allow-listed audited app location. Absent on older hosts. */
    appLocationReveal: z.boolean().optional(),
  }),
  icloud: z.enum(['unavailable']),
  /**
   * True when the host is configured with an operator-owned AI advisor
   * (`packages/contract/src/ai.ts`). Absent on older hosts, which never
   * advertise it.
   */
  ai: z.boolean().optional(),
  /**
   * Memory view and process control (crates/memory). Absent on older hosts and
   * on machine-mode hosts that never pass the allow flags, in which case the
   * Memory view stays hidden.
   */
  memory: z
    .strictObject({
      /** snapshot/history/candidates/audit readable. */
      sample: z.boolean(),
      /** plan/execute available to this session's host config. */
      terminate: z.boolean(),
      /** AppKit quit works (macOS + appkit feature). */
      gracefulAppQuit: z.boolean(),
      /** desktop-only "Reopen" from audit. */
      reopen: z.boolean(),
      platform: z.enum(MEMORY_PLATFORMS),
    })
    .optional(),
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
