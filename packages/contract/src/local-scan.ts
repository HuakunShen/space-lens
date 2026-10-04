import { z } from 'zod'

const UintSchema = z.number().int().nonnegative()
const PathSchema = z.string().min(1).max(4096)

export const LocalNodeStateSchema = z.enum(['complete', 'partial', 'skipped'])
export type LocalNodeState = z.infer<typeof LocalNodeStateSchema>

export const LocalNodeMetadataShape = {
  logicalSize: UintSchema.optional(),
  isDirectory: z.boolean().optional(),
  scanState: LocalNodeStateSchema.optional(),
  skipReason: z.string().nullable().optional(),
} as const

export interface LocalScanNode {
  name: string
  path: string
  size: number
  children: LocalScanNode[]
  depth: number
  ignored: boolean
  collapsed: boolean
  logicalSize: number
  isDirectory: boolean
  scanState: LocalNodeState
  skipReason: string | null
}

const localScanNodeShape = {
  name: z.string(),
  path: PathSchema,
  size: UintSchema,
  depth: UintSchema,
  ignored: z.boolean(),
  collapsed: z.boolean(),
  logicalSize: UintSchema,
  isDirectory: z.boolean(),
  scanState: LocalNodeStateSchema,
  skipReason: z.string().nullable(),
}

export const LocalScanNodeSchema: z.ZodType<LocalScanNode> = z.strictObject({
  ...localScanNodeShape,
  children: z.array(z.lazy(() => LocalScanNodeSchema)),
})

/** Streaming frames carry one node, never a recursive subtree. */
export const LocalScanStreamNodeSchema = z.strictObject({
  ...localScanNodeShape,
  children: z.array(z.never()).length(0),
})
export type LocalScanStreamNode = z.infer<typeof LocalScanStreamNodeSchema>

export const ScanCoverageSchema = z.strictObject({
  mode: z.literal('local-only'),
  protection: z.enum(['macos-no-materialization', 'metadata-only']),
  sizeMetric: z.literal('allocated'),
  logicalBytes: UintSchema,
  files: UintSchema,
  directories: UintSchema,
  skippedCount: UintSchema,
  deniedCount: UintSchema,
  issueCount: UintSchema,
  issuesTruncated: z.boolean(),
  issues: z.array(z.strictObject({ path: PathSchema, reason: z.string(), message: z.string() })).max(1000),
  elapsedMs: UintSchema,
})
export type ScanCoverage = z.infer<typeof ScanCoverageSchema>

export const ScanVolumeSchema = z.strictObject({
  path: PathSchema,
  totalBytes: UintSchema,
  availableBytes: UintSchema,
  freeBytes: UintSchema,
  isLocal: z.boolean(),
})
export type ScanVolume = z.infer<typeof ScanVolumeSchema>

export const LocalScanProgressSchema = z.strictObject({
  currentPath: PathSchema.nullable(),
  bytesScanned: UintSchema,
  entriesScanned: UintSchema,
  files: UintSchema,
  directories: UintSchema,
  skippedCount: UintSchema,
  deniedCount: UintSchema,
  elapsedMs: UintSchema,
})
export type LocalScanProgress = z.infer<typeof LocalScanProgressSchema>

export const LocalScanReportSchema = z.strictObject({
  nodes: z.array(LocalScanNodeSchema),
  coverage: ScanCoverageSchema,
  volumes: z.array(ScanVolumeSchema),
})
export type LocalScanReport = z.infer<typeof LocalScanReportSchema>

export const LocalScanMessageSchema = z.discriminatedUnion('type', [
  z.strictObject({ type: z.literal('node'), node: LocalScanStreamNodeSchema }),
  z.strictObject({ type: z.literal('progress'), progress: LocalScanProgressSchema }),
  z.strictObject({ type: z.literal('done'), report: LocalScanReportSchema }),
])
export type LocalScanMessage = z.infer<typeof LocalScanMessageSchema>
