import { z } from 'zod'

export const IgnoredModeSchema = z.enum(['summarize', 'exclude'])
export type IgnoredMode = z.infer<typeof IgnoredModeSchema>

export const SortModeSchema = z.enum(['size', 'name', 'path'])
export type SortMode = z.infer<typeof SortModeSchema>

const IsoDateTimeSchema = z.iso.datetime()
const UintSchema = z.number().int().nonnegative()
const PathSchema = z.string().min(1).max(4096)
export const ScanIdSchema = z.string().regex(/^scan_[0-9a-zA-Z]{8,64}$/)
export const NodeIdSchema = z.string().min(1).max(128)

export const ScanStartRequestSchema = z.strictObject({
  /** Absolute directories to scan. At least one, at most 16. */
  paths: z.array(PathSchema).min(1).max(16),
  ignoreHidden: z.boolean().default(false),
  respectGitignore: z.boolean().default(true),
  ignoredMode: IgnoredModeSchema.default('summarize'),
  label: z.string().max(200).optional(),
})

export type ScanStartRequest = z.infer<typeof ScanStartRequestSchema>

export const ScanSessionSchema = z.strictObject({
  scanId: ScanIdSchema,
  rootIds: z.array(NodeIdSchema),
  createdAt: IsoDateTimeSchema,
  label: z.string().nullable(),
})

export type ScanSession = z.infer<typeof ScanSessionSchema>

export const ScanStateSchema = z.enum(['idle', 'scanning', 'ready', 'cancelled', 'failed'])
export type ScanState = z.infer<typeof ScanStateSchema>

export const ScanStatusSchema = z.strictObject({
  scanId: ScanIdSchema,
  state: ScanStateSchema,
  message: z.string(),
  /**
   * The engine exposes no progress callback, so a host that cannot know says
   * `null` instead of inventing a percentage.
   */
  progress: z.number().min(0).max(1).nullable(),
  /** Always null today; reserved for hosts whose engine reports the current path. */
  currentPath: z.string().nullable(),
  bytesScanned: UintSchema,
  entriesScanned: UintSchema,
  rootIds: z.array(NodeIdSchema),
  label: z.string().nullable(),
  updatedAt: IsoDateTimeSchema,
})

export type ScanStatus = z.infer<typeof ScanStatusSchema>

export const ScanListResponseSchema = z.strictObject({
  scans: z.array(ScanStatusSchema),
})

export type ScanListResponse = z.infer<typeof ScanListResponseSchema>

export const TreeNodeSummarySchema = z.strictObject({
  id: NodeIdSchema,
  name: z.string(),
  path: PathSchema,
  size: UintSchema,
  depth: UintSchema,
  ignored: z.boolean(),
  collapsed: z.boolean(),
  hasChildren: z.boolean(),
  childCount: UintSchema,
})

export type TreeNodeSummary = z.infer<typeof TreeNodeSummarySchema>

export interface TreeSliceNode extends TreeNodeSummary {
  children: TreeSliceNode[]
  omittedBytes: number
  omittedCount: number
}

const treeNodeSummaryShape = {
  id: NodeIdSchema,
  name: z.string(),
  path: PathSchema,
  size: UintSchema,
  depth: UintSchema,
  ignored: z.boolean(),
  collapsed: z.boolean(),
  hasChildren: z.boolean(),
  childCount: UintSchema,
} as const

export const TreeSliceNodeSchema: z.ZodType<TreeSliceNode> = z.strictObject({
  ...treeNodeSummaryShape,
  children: z.array(z.lazy(() => TreeSliceNodeSchema)),
  omittedBytes: UintSchema,
  omittedCount: UintSchema,
})

export const TreeSliceRequestSchema = z.strictObject({
  scanId: ScanIdSchema,
  nodeId: NodeIdSchema,
  depth: z.number().int().min(0).max(24),
  maxChildrenPerNode: z.number().int().min(1).max(512),
})

export type TreeSliceRequest = z.infer<typeof TreeSliceRequestSchema>

export const TreeSliceSchema = z.strictObject({
  scanId: ScanIdSchema,
  focusNode: TreeNodeSummarySchema,
  ancestors: z.array(TreeNodeSummarySchema),
  tree: TreeSliceNodeSchema,
  totalSize: UintSchema,
  truncated: z.boolean(),
  omittedBytes: UintSchema,
  omittedCount: UintSchema,
  generatedAt: IsoDateTimeSchema,
})

export type TreeSlice = z.infer<typeof TreeSliceSchema>

export const ChildrenPageRequestSchema = z.strictObject({
  scanId: ScanIdSchema,
  nodeId: NodeIdSchema,
  offset: UintSchema,
  limit: z.number().int().min(1).max(1000),
  sort: SortModeSchema,
})

export type ChildrenPageRequest = z.infer<typeof ChildrenPageRequestSchema>

export const ChildrenPageSchema = z.strictObject({
  scanId: ScanIdSchema,
  nodeId: NodeIdSchema,
  items: z.array(TreeNodeSummarySchema),
  offset: UintSchema,
  limit: z.number().int().min(1).max(1000),
  total: UintSchema,
  sort: SortModeSchema,
})

export type ChildrenPage = z.infer<typeof ChildrenPageSchema>

export const ScanCancelRequestSchema = z.strictObject({})

export type ScanCancelRequest = z.infer<typeof ScanCancelRequestSchema>

/**
 * A scan target the UI may offer. Hosts derive preset targets from their
 * configured roots; `recent` targets are client-side history and never served
 * by the host.
 */
export const ScanTargetSchema = z.strictObject({
  id: z.string().min(1).max(128),
  label: z.string().min(1).max(200),
  path: PathSchema,
  kind: z.enum(['volume', 'folder', 'multi-folder']),
  description: z.string().max(500).default(''),
  size: UintSchema.default(0),
  used: UintSchema.optional(),
  source: z.enum(['preset', 'recent']),
  removable: z.boolean().default(false),
  lastScannedAt: IsoDateTimeSchema.optional(),
})

export type ScanTarget = z.infer<typeof ScanTargetSchema>

export const RootsResponseSchema = z.strictObject({
  roots: z.array(ScanTargetSchema),
})

export type RootsResponse = z.infer<typeof RootsResponseSchema>
