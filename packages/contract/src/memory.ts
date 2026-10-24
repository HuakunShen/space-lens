/**
 * Memory view and process-control contract (§4.1/§4.3 of
 * docs/memory-cleaner-plan.md): the Zod mirror of the Rust wire types in
 * `crates/memory`. Field names are camelCase; byte counts and microsecond
 * timestamps are unsigned integers below 2^53. The literal arrays below are
 * written exactly once and reused by the capability view in ./capabilities.ts.
 *
 * `packages/contract/src/apps.ts` already exports `AuditIdSchema` (audit
 * generations), so this module's audit id is `MemoryAuditIdSchema`.
 */
import { z } from 'zod'

const Uint = z.number().int().nonnegative()
const Iso = z.iso.datetime()
const Name = z.string().max(256)

export const MEMORY_PLATFORMS = ['macos', 'linux', 'windows', 'unsupported'] as const
export const PRESSURE_LEVELS = ['normal', 'warning', 'critical', 'unknown'] as const
export const PRESSURE_SOURCES = ['kernel', 'derived', 'unavailable'] as const
export const PROCESS_KINDS = ['app', 'helper', 'agent', 'cli', 'system', 'other'] as const
/** Scheduler states; the platforms disagree on details, anything else is `unknown`. */
export const PROCESS_STATES = ['running', 'sleeping', 'idle', 'stopped', 'zombie', 'unknown'] as const
export type ProcessState = (typeof PROCESS_STATES)[number]
export const PROTECTIONS = [
  'kernel',
  'init',
  'sessionCritical',
  'systemComponent',
  'selfOrAncestor',
  'selfChild',
  'otherUser',
  'unidentified',
  'zombie',
] as const
export const SNAPSHOT_NOTES = [
  'cpuWarmingUp',
  'restrictedProcesses',
  'breakdownUnavailable',
  'pressureDerived',
  'truncated',
] as const
export const TERMINATE_MODES = ['graceful', 'force'] as const
export const TERMINATE_ORIGINS = ['manual', 'freeUp', 'advisor'] as const
export const TERMINATE_METHODS = ['appQuit', 'sigterm', 'sigkill'] as const
export const RISK_FLAGS = [
  'appMayPromptToSave',
  'unsavedWorkMayBeLost',
  'endsChildProcesses',
  'backgroundAgent',
  'recentlyUsed',
  'restarts',
] as const
export const REFUSAL_CODES = ['protected', 'notFound', 'identityChanged', 'duplicate', 'tooMany', 'frontmost'] as const
export const TERMINATE_STATUSES = [
  'exited',
  'stillRunning',
  'alreadyGone',
  'identityChanged',
  'refused',
  'failed',
] as const
export const REASON_CODES = ['large', 'idle', 'background', 'growing', 'manyProcesses'] as const

/** FNV-1a 64 of the group key, lowercase hex. */
export const GroupIdSchema = z.string().regex(/^grp_[0-9a-f]{16}$/)
export const TerminatePlanIdSchema = z.string().regex(/^tplan_[0-9a-zA-Z]{16}$/)
export const MemoryAuditIdSchema = z.string().regex(/^aud_[0-9a-zA-Z]{16}$/)

const PlatformSchema = z.enum(MEMORY_PLATFORMS)
const PressureLevelSchema = z.enum(PRESSURE_LEVELS)
const PressureSourceSchema = z.enum(PRESSURE_SOURCES)
const ProcessKindSchema = z.enum(PROCESS_KINDS)
const ProcessStateSchema = z.enum(PROCESS_STATES)
const ProtectionSchema = z.enum(PROTECTIONS)
const SnapshotNoteSchema = z.enum(SNAPSHOT_NOTES)
const TerminateModeSchema = z.enum(TERMINATE_MODES)
const TerminateOriginSchema = z.enum(TERMINATE_ORIGINS)
const TerminateMethodSchema = z.enum(TERMINATE_METHODS)
const RiskFlagSchema = z.enum(RISK_FLAGS)
const RefusalCodeSchema = z.enum(REFUSAL_CODES)
const TerminateStatusSchema = z.enum(TERMINATE_STATUSES)
const ReasonCodeSchema = z.enum(REASON_CODES)
/** f32 percentages: nonnegative where the range is open, 0..100 where it is a share. */
const RatioSchema = z.number().nonnegative()
const PercentSchema = z.number().min(0).max(100)

export const MemoryBreakdownSchema = z.strictObject({
  totalBytes: Uint,
  usedBytes: Uint,
  freeBytes: Uint,
  availableBytes: Uint.nullable(),
  appBytes: Uint.nullable(),
  wiredBytes: Uint.nullable(),
  compressedBytes: Uint.nullable(),
  cachedFilesBytes: Uint.nullable(),
  swapTotalBytes: Uint,
  swapUsedBytes: Uint,
})
export type MemoryBreakdown = z.infer<typeof MemoryBreakdownSchema>

export const MemoryPressureSchema = z.strictObject({
  level: PressureLevelSchema,
  source: PressureSourceSchema,
  availablePercent: PercentSchema.nullable(),
})
export type MemoryPressure = z.infer<typeof MemoryPressureSchema>

export const ProcessRowSchema = z.strictObject({
  pid: Uint,
  ppid: Uint.nullable(),
  startTimeUs: Uint,
  name: Name,
  ownedByCurrentUser: z.boolean(),
  groupId: GroupIdSchema,
  kind: ProcessKindSchema,
  cpuPercent: RatioSchema.nullable(),
  footprintBytes: Uint.nullable(),
  residentBytes: Uint.nullable(),
  virtualBytes: Uint.nullable(),
  restricted: z.boolean(),
  protection: ProtectionSchema.nullable(),
  /** Full argv joined with spaces; null when the kernel does not expose it. */
  command: z.string().max(4096).nullable(),
  /** Owning account name, when resolvable. */
  user: z.string().max(128).nullable(),
  state: ProcessStateSchema,
  /** Thread count, when the platform reports it. */
  threads: Uint.nullable(),
  /** Listening TCP ports, sorted ascending (at most 32). */
  listeningPorts: z.array(Uint).max(32),
})
export type ProcessRow = z.infer<typeof ProcessRowSchema>

export const ProcessGroupSchema = z.strictObject({
  groupId: GroupIdSchema,
  name: Name,
  bundleId: z.string().nullable(),
  bundlePath: z.string().nullable(),
  kind: ProcessKindSchema,
  mainPid: Uint,
  pids: z.array(Uint),
  cpuPercent: RatioSchema.nullable(),
  footprintBytes: Uint,
  residentBytes: Uint,
  frontmost: z.boolean(),
  lastFrontmostAt: Iso.nullable(),
  startedAt: Iso,
  protection: ProtectionSchema.nullable(),
})
export type ProcessGroup = z.infer<typeof ProcessGroupSchema>

export const MemorySnapshotSchema = z.strictObject({
  sequence: Uint,
  sampledAt: Iso,
  intervalMs: Uint,
  platform: PlatformSchema,
  breakdown: MemoryBreakdownSchema,
  pressure: MemoryPressureSchema,
  groups: z.array(ProcessGroupSchema).max(5000),
  processes: z.array(ProcessRowSchema).max(20000),
  notes: z.array(SnapshotNoteSchema),
})
export type MemorySnapshot = z.infer<typeof MemorySnapshotSchema>

export const TerminateTargetSchema = z.discriminatedUnion('kind', [
  z.strictObject({ kind: z.literal('group'), groupId: GroupIdSchema }),
  z.strictObject({ kind: z.literal('process'), pid: Uint, startTimeUs: Uint }),
])
export type TerminateTarget = z.infer<typeof TerminateTargetSchema>

export const TerminatePlanRequestSchema = z.strictObject({
  targets: z.array(TerminateTargetSchema).min(1).max(20),
  mode: TerminateModeSchema,
  origin: TerminateOriginSchema,
})
export type TerminatePlanRequest = z.infer<typeof TerminatePlanRequestSchema>

export const TerminatePlanEntrySchema = z.strictObject({
  pid: Uint,
  startTimeUs: Uint,
  name: Name,
  groupId: GroupIdSchema,
  footprintBytes: Uint.nullable(),
  method: TerminateMethodSchema,
  risks: z.array(RiskFlagSchema),
  descendantCount: Uint,
})
export type TerminatePlanEntry = z.infer<typeof TerminatePlanEntrySchema>

export const TerminateRefusalSchema = z.strictObject({
  pid: Uint.nullable(),
  groupId: GroupIdSchema.nullable(),
  name: Name,
  code: RefusalCodeSchema,
  protection: ProtectionSchema.nullable(),
})
export type TerminateRefusal = z.infer<typeof TerminateRefusalSchema>

export const TerminatePlanSchema = z.strictObject({
  planId: TerminatePlanIdSchema,
  mode: TerminateModeSchema,
  origin: TerminateOriginSchema,
  entries: z.array(TerminatePlanEntrySchema).max(64),
  refused: z.array(TerminateRefusalSchema),
  expectedReleaseBytes: Uint,
  createdAt: Iso,
  expiresAt: Iso,
})
export type TerminatePlan = z.infer<typeof TerminatePlanSchema>

export const TerminateExecuteRequestSchema = z.strictObject({
  planId: TerminatePlanIdSchema,
  /** Must be the literal `true`. A default, a missing field, or `false` refuses. */
  confirm: z.literal(true),
})
export type TerminateExecuteRequest = z.infer<typeof TerminateExecuteRequestSchema>

export const TerminateResultSchema = z.strictObject({
  pid: Uint,
  startTimeUs: Uint,
  name: Name,
  status: TerminateStatusSchema,
  method: TerminateMethodSchema,
  errno: z.number().int().nullable(),
})
export type TerminateResult = z.infer<typeof TerminateResultSchema>

export const TerminateOutcomeSchema = z.strictObject({
  planId: TerminatePlanIdSchema,
  results: z.array(TerminateResultSchema),
  releasedBytesEstimate: Uint,
  auditIds: z.array(MemoryAuditIdSchema),
})
export type TerminateOutcome = z.infer<typeof TerminateOutcomeSchema>

export const CandidateReasonSchema = z.strictObject({
  code: ReasonCodeSchema,
  bytes: Uint.nullable(),
  minutes: Uint.nullable(),
  cpuPercent: RatioSchema.nullable(),
  count: Uint.nullable(),
  fromBytes: Uint.nullable(),
})
export type CandidateReason = z.infer<typeof CandidateReasonSchema>

export const FreeUpCandidateSchema = z.strictObject({
  groupId: GroupIdSchema,
  name: Name,
  footprintBytes: Uint,
  reasons: z.array(CandidateReasonSchema),
  risks: z.array(RiskFlagSchema),
  preselected: z.boolean(),
})
export type FreeUpCandidate = z.infer<typeof FreeUpCandidateSchema>

export const FreeUpCandidatesSchema = z.strictObject({
  sequence: Uint,
  pressure: MemoryPressureSchema,
  observedMinutes: Uint,
  candidates: z.array(FreeUpCandidateSchema).max(8),
  potentialReleaseBytes: Uint,
})
export type FreeUpCandidates = z.infer<typeof FreeUpCandidatesSchema>

export const CandidatesRequestSchema = z.strictObject({
  exclude: z.array(GroupIdSchema).max(200),
})
export type CandidatesRequest = z.infer<typeof CandidatesRequestSchema>

export const SystemPointSchema = z.strictObject({
  at: Iso,
  usedBytes: Uint,
  compressedBytes: Uint.nullable(),
  swapUsedBytes: Uint,
  pressure: PressureLevelSchema,
})
export type SystemPoint = z.infer<typeof SystemPointSchema>

export const GroupSeriesSchema = z.strictObject({
  groupId: GroupIdSchema,
  name: Name,
  footprintBytes: z.array(Uint.nullable()),
})
export type GroupSeries = z.infer<typeof GroupSeriesSchema>

export const MemoryHistorySchema = z.strictObject({
  stepMs: Uint,
  points: z.array(SystemPointSchema).max(600),
  groups: z.array(GroupSeriesSchema).max(8),
})
export type MemoryHistory = z.infer<typeof MemoryHistorySchema>

export const HistoryRequestSchema = z.strictObject({
  groupIds: z.array(GroupIdSchema).max(8),
})
export type HistoryRequest = z.infer<typeof HistoryRequestSchema>

export const AuditRecordSchema = z.strictObject({
  auditId: MemoryAuditIdSchema,
  at: Iso,
  origin: TerminateOriginSchema,
  mode: TerminateModeSchema,
  method: TerminateMethodSchema,
  pid: Uint,
  startTimeUs: Uint,
  name: Name,
  groupName: Name,
  bundleId: z.string().nullable(),
  status: TerminateStatusSchema,
  footprintBytes: Uint.nullable(),
})
export type AuditRecord = z.infer<typeof AuditRecordSchema>

export const AuditLogSchema = z.strictObject({
  records: z.array(AuditRecordSchema).max(200),
})
export type AuditLog = z.infer<typeof AuditLogSchema>
