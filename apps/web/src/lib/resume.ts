import { z } from 'zod'
import { AuditIdSchema, ScanSessionSchema, ScanStartRequestSchema, NodeIdSchema } from '@space-lens/contract'

export const ACTIVE_SCAN_KEY = 'spacelens.activeScan'

/**
 * A10: the virtual `app-storage:` identifier is never persisted as a scan
 * input. Path sessions persist `paths`; app sessions persist the app
 * identity + pinned generation and re-resolve locations from the current
 * audit on restore.
 */
const ActiveScanSchema = ScanSessionSchema.pick({ scanId: true }).extend({
  paths: ScanStartRequestSchema.shape.paths.optional(),
  app: z
    .strictObject({
      appId: z.string().min(1).max(128),
      auditId: AuditIdSchema,
    })
    .optional(),
  focusNodeId: NodeIdSchema.optional(),
})
export type ActiveScan = ReturnType<typeof ActiveScanSchema.parse>

export function parseActiveScan(raw: string | null): ActiveScan | null {
  if (!raw || raw.length > 70_000) return null
  try {
    const parsed = ActiveScanSchema.safeParse(JSON.parse(raw))
    if (!parsed.success) return null
    // At least one input form must be present; app scans carry no paths.
    if (!parsed.data.paths && !parsed.data.app) return null
    return parsed.data
  } catch {
    return null
  }
}
