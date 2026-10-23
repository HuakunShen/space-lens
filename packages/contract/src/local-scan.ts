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
  /** `skippedCount` split by skip reason; the values sum to it. Absent from older reports. */
  skippedByReason: z.record(z.string().min(1).max(64), UintSchema).optional(),
  deniedCount: UintSchema,
  issueCount: UintSchema,
  issuesTruncated: z.boolean(),
  issues: z.array(z.strictObject({ path: PathSchema, reason: z.string(), message: z.string() })).max(1000),
  elapsedMs: UintSchema,
})
export type ScanCoverage = z.infer<typeof ScanCoverageSchema>

/**
 * Skip reasons that leave no space unmeasured: a symlink is never followed and
 * holds no file data; sockets and devices hold none either; a duplicate
 * directory alias and a separately selected root are measured once elsewhere;
 * an online-only (dataless) file uses no local space.
 */
export const BY_DESIGN_SKIP_REASONS: readonly string[] = [
  'symlink',
  'special-file',
  'duplicate-directory',
  'separate-root',
  'dataless',
]

/** Skipped entries whose size this scan could not measure. */
export function unmeasuredSkipCount(coverage: ScanCoverage): number {
  const byReason = coverage.skippedByReason ?? {}
  const byDesign = BY_DESIGN_SKIP_REASONS.reduce((total, reason) => total + (byReason[reason] ?? 0), 0)
  return Math.max(0, coverage.skippedCount - byDesign)
}

export const ScanVolumeSchema = z.strictObject({
  path: PathSchema,
  totalBytes: UintSchema,
  availableBytes: UintSchema,
  freeBytes: UintSchema,
  isLocal: z.boolean(),
})
export type ScanVolume = z.infer<typeof ScanVolumeSchema>

export const ApfsVolumeUsageSchema = z.strictObject({
  name: z.string().max(256),
  /** APFS role (`Data`, `System`, `VM`, `Preboot`, `Recovery`, `Update`, ...) or `None`. */
  role: z.string().min(1).max(64),
  device: z.string().min(1).max(64),
  inUseBytes: UintSchema,
})
export type ApfsVolumeUsage = z.infer<typeof ApfsVolumeUsageSchema>

/**
 * APFS container accounting captured when a scan finishes, so the UI can
 * reconcile the measured bytes with the disk's own "used" figure. macOS does
 * not size individual snapshots, so they are counted, not sized.
 */
export const StorageBreakdownSchema = z.strictObject({
  container: z.string().min(1).max(64),
  capacityBytes: UintSchema,
  /** Everything allocated in the container: all volumes, snapshots and purgeable files. */
  inUseBytes: UintSchema,
  /** Space macOS can free on demand (local snapshots, cached iCloud files). */
  purgeableBytes: UintSchema.nullable(),
  /** Bytes this scan measured under roots inside the container. */
  scannedBytes: UintSchema,
  volumes: z.array(ApfsVolumeUsageSchema).max(64),
  snapshots: z
    .strictObject({
      volume: z.string().min(1).max(64),
      count: UintSchema,
      timeMachineCount: UintSchema,
      /** Local time of the oldest Time Machine snapshot, `YYYY-MM-DDTHH:MM:SS`. */
      oldestTimeMachine: z
        .string()
        .regex(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}$/)
        .nullable(),
    })
    .nullable(),
})
export type StorageBreakdown = z.infer<typeof StorageBreakdownSchema>

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
