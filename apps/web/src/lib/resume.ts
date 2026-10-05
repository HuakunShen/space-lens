import { ScanSessionSchema, ScanStartRequestSchema, NodeIdSchema } from '@space-lens/contract'

export const ACTIVE_SCAN_KEY = 'spacelens.activeScan'
const ActiveScanSchema = ScanSessionSchema.pick({ scanId: true }).extend({
  paths: ScanStartRequestSchema.shape.paths,
  focusNodeId: NodeIdSchema.optional(),
})
export type ActiveScan = ReturnType<typeof ActiveScanSchema.parse>

export function parseActiveScan(raw: string | null): ActiveScan | null {
  if (!raw || raw.length > 70_000) return null
  try {
    const parsed = ActiveScanSchema.safeParse(JSON.parse(raw))
    return parsed.success ? parsed.data : null
  } catch {
    return null
  }
}
