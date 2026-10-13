import { z } from 'zod'

export const AppAuditClassificationSchema = z.enum([
  'application',
  'cache',
  'versions',
  'logs',
  're-downloadable',
  'user-data',
  'support',
])
export type AppAuditClassification = z.infer<typeof AppAuditClassificationSchema>

export const AppAuditSourceSchema = z.enum(['catalog', 'heuristic'])
export type AppAuditSource = z.infer<typeof AppAuditSourceSchema>

export const AppAuditLocationSchema = z.strictObject({
  path: z.string().min(1).max(4096),
  label: z.string().nullable(),
  classification: AppAuditClassificationSchema,
  purgeable: z.boolean(),
  comment: z.string().nullable(),
  size: z.number().int().nonnegative(),
  source: AppAuditSourceSchema,
})
export type AppAuditLocation = z.infer<typeof AppAuditLocationSchema>

export const AppAuditEntrySchema = z.strictObject({
  id: z.string().min(1).max(128),
  name: z.string().min(1).max(256),
  kind: z.enum(['app', 'cli']),
  source: AppAuditSourceSchema,
  bundleId: z.string().nullable(),
  appPath: z.string().nullable(),
  installedSize: z.number().int().nonnegative(),
  totalSize: z.number().int().nonnegative(),
  locations: z.array(AppAuditLocationSchema).max(200),
})
export type AppAuditEntry = z.infer<typeof AppAuditEntrySchema>

export const AppAuditRequestSchema = z.strictObject({
  minSize: z.number().int().nonnegative().default(0),
  includeHeuristic: z.boolean().default(true),
  limit: z.number().int().min(1).max(1000).default(500),
})
export type AppAuditRequest = z.infer<typeof AppAuditRequestSchema>

export const AppAuditReportSchema = z.strictObject({
  platform: z.string().min(1),
  generatedAt: z.iso.datetime(),
  apps: z.array(AppAuditEntrySchema).max(1000),
  totalApps: z.number().int().nonnegative(),
  truncated: z.boolean(),
  /** Opaque per-generation authorization token minted by the desktop host.
   * Optional so older hosts that predate V2 still parse. */
  auditId: z
    .string()
    .regex(/^audit_[0-9a-zA-Z]{8,64}$/)
    .optional(),
})
export type AppAuditReport = z.infer<typeof AppAuditReportSchema>

export const AuditIdSchema = z.string().regex(/^audit_[0-9a-zA-Z]{8,64}$/)
export type AuditId = z.infer<typeof AuditIdSchema>

export const AppScanOriginSchema = z.strictObject({
  appId: z.string().min(1).max(128),
  appName: z.string().min(1).max(256),
  auditId: AuditIdSchema,
})
export type AppScanOrigin = z.infer<typeof AppScanOriginSchema>

export const AppScanStartRequestSchema = z.strictObject({
  auditId: AuditIdSchema,
  appId: z.string().min(1).max(128),
})
export type AppScanStartRequest = z.infer<typeof AppScanStartRequestSchema>

// Client-only recent (no wire, localStorage key spacelens.recentAppScans, max 5)
export const RecentAppScanSchema = z.strictObject({
  appId: z.string().min(1).max(128),
  appName: z.string().min(1).max(256),
  auditId: AuditIdSchema,
  lastScannedAt: z.iso.datetime(),
})
export type RecentAppScan = z.infer<typeof RecentAppScanSchema>

export const AppIconRequestSchema = z.strictObject({ appPath: z.string().min(1).max(4096) })
export type AppIconRequest = z.infer<typeof AppIconRequestSchema>

export const AppIconSchema = z.strictObject({ dataUrl: z.string().min(1) })
export type AppIcon = z.infer<typeof AppIconSchema>
