import { z } from 'zod'
import { PROBLEM_CODES } from './problem.ts'
import { ScanIdSchema } from './scan.ts'

const UintSchema = z.number().int().nonnegative()
const IsoDateTimeSchema = z.iso.datetime()
const PathSchema = z.string().min(1).max(4096)

export const PlanIdSchema = z.string().regex(/^plan_[0-9a-zA-Z]{8,64}$/)

export const CleanupPlanRequestSchema = z.strictObject({
  scanId: ScanIdSchema,
  nodeIds: z.array(z.string().min(1).max(128)).min(1).max(100000),
})

export type CleanupPlanRequest = z.infer<typeof CleanupPlanRequestSchema>

/**
 * Fingerprint captured at plan time. Execution re-verifies every entry and
 * refuses with `StalePlan` when the fingerprint no longer matches, because
 * directory listings alone are not proof that content is unchanged.
 */
export const EntryFingerprintSchema = z.strictObject({
  size: UintSchema,
  mtimeMs: z.number(),
})

export type EntryFingerprint = z.infer<typeof EntryFingerprintSchema>

export const PlanEntrySchema = z.strictObject({
  path: PathSchema,
  size: UintSchema,
  reason: z.string(),
  preset: z.string().nullable(),
  ignored: z.boolean(),
  fingerprint: EntryFingerprintSchema,
})

export type PlanEntry = z.infer<typeof PlanEntrySchema>

/**
 * V1 web/extension cleanup is trash-only. Permanent deletion stays a CLI/TUI
 * concern and is never reachable through this contract.
 */
export const CLEANUP_MODES = ['none', 'trash'] as const

export const CleanupPlanSchema = z.strictObject({
  planId: PlanIdSchema,
  scanId: ScanIdSchema,
  mode: z.enum(CLEANUP_MODES),
  entries: z.array(PlanEntrySchema),
  totalSize: UintSchema,
  errors: z.array(z.string()),
  createdAt: IsoDateTimeSchema,
  expiresAt: IsoDateTimeSchema,
})

export type CleanupPlan = z.infer<typeof CleanupPlanSchema>

export const CleanupExecuteRequestSchema = z.strictObject({
  planId: PlanIdSchema,
  /** Must be the literal `true`. A default, a missing field, or `false` refuses. */
  confirm: z.literal(true),
})

export type CleanupExecuteRequest = z.infer<typeof CleanupExecuteRequestSchema>

export const TrashedEntrySchema = z.strictObject({
  path: PathSchema,
  size: UintSchema,
})

export const CleanupOutcomeSchema = z.strictObject({
  trashed: z.array(TrashedEntrySchema),
  bytesFreed: UintSchema,
  failed: z.array(
    z.strictObject({
      path: PathSchema,
      code: z.enum(PROBLEM_CODES),
      message: z.string(),
    }),
  ),
})

export type CleanupOutcome = z.infer<typeof CleanupOutcomeSchema>

/**
 * A staged cleanup entry. The collector lives in the client; the host only
 * ever receives nodeIds at plan time. This type exists so every frontend and
 * the host agree on the shape when exchanging or persisting the collector.
 */
export const CollectorEntrySchema = z.strictObject({
  id: z.string().min(1).max(128),
  scanId: ScanIdSchema,
  nodeId: z.string().min(1).max(128),
  path: PathSchema,
  name: z.string().min(1).max(1024),
  size: UintSchema,
  addedAt: IsoDateTimeSchema,
})

export type CollectorEntry = z.infer<typeof CollectorEntrySchema>
