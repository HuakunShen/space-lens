import { z } from 'zod'
import { ScanIdSchema, TreeNodeSummarySchema, NodeIdSchema } from './scan.ts'

const UintSchema = z.number().int().nonnegative()
const LimitSchema = z.number().int().min(1).max(1000)

export const DiscoveryKindSchema = z.enum(['large-files', 'caches', 'gitignored'])
export type DiscoveryKind = z.infer<typeof DiscoveryKindSchema>

export const DiscoveryRequestSchema = z.strictObject({
  scanId: ScanIdSchema,
  kind: DiscoveryKindSchema,
  minSize: UintSchema.default(0),
  offset: UintSchema.default(0),
  limit: LimitSchema.default(200),
})
export type DiscoveryRequest = z.infer<typeof DiscoveryRequestSchema>

export const DiscoveryItemSchema = z.strictObject({
  node: TreeNodeSummarySchema,
  parentId: NodeIdSchema.nullable(),
  category: z.string(),
  isDirectory: z.boolean(),
})
export type DiscoveryItem = z.infer<typeof DiscoveryItemSchema>

export const DiscoveryPageSchema = z.strictObject({
  scanId: ScanIdSchema,
  kind: DiscoveryKindSchema,
  items: z.array(DiscoveryItemSchema),
  total: UintSchema,
  totalSize: UintSchema,
  offset: UintSchema,
  limit: LimitSchema,
})
export type DiscoveryPage = z.infer<typeof DiscoveryPageSchema>
