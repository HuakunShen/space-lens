import { z } from 'zod'
import { AiChatRequestSchema, AiConfigViewSchema } from './ai.ts'
import { CapabilitiesSchema, HealthSchema } from './capabilities.ts'
import {
  AppCleanupExecuteRequestSchema,
  AppCleanupPlanRequestSchema,
  AppCleanupPlanSchema,
  CleanupExecuteRequestSchema,
  CleanupOutcomeSchema,
  CleanupPlanRequestSchema,
  CleanupPlanSchema,
} from './cleanup.ts'
import { EventEnvelopeSchema } from './events.ts'
import {
  AuditLogSchema,
  AuditRecordSchema,
  CandidateReasonSchema,
  CandidatesRequestSchema,
  FreeUpCandidateSchema,
  FreeUpCandidatesSchema,
  GroupSeriesSchema,
  HistoryRequestSchema,
  MemoryBreakdownSchema,
  MemoryHistorySchema,
  MemoryPressureSchema,
  MemorySnapshotSchema,
  ProcessGroupSchema,
  ProcessRowSchema,
  SystemPointSchema,
  TerminateExecuteRequestSchema,
  TerminateOutcomeSchema,
  TerminatePlanEntrySchema,
  TerminatePlanRequestSchema,
  TerminatePlanSchema,
  TerminateRefusalSchema,
  TerminateResultSchema,
  TerminateTargetSchema,
} from './memory.ts'
import { DiscoveryKindSchema, DiscoveryRequestSchema, DiscoveryItemSchema, DiscoveryPageSchema } from './discovery.ts'
import {
  AppAuditRequestSchema,
  AppAuditReportSchema,
  AppIconRequestSchema,
  AppIconSchema,
  AppScanOriginSchema,
  AppScanStartRequestSchema,
  RecentAppScanSchema,
} from './apps.ts'
import {
  LocalScanReportSchema,
  LocalScanNodeSchema,
  LocalScanProgressSchema,
  LocalScanMessageSchema,
  LocalScanStreamNodeSchema,
  ScanCoverageSchema,
  ScanVolumeSchema,
} from './local-scan.ts'
import { ProblemSchema } from './problem.ts'
import { RevealPathRequestSchema, RevealPathResultSchema } from './reveal.ts'
import { ScanStartRequestSchema, ScanStatusSchema, TreeSliceSchema, TreeSliceRequestSchema } from './scan.ts'
import { SessionExchangeRequestSchema, SessionSchema } from './session.ts'
import { API_MAJOR, CONTRACT_VERSION } from './version.ts'

/**
 * The named public surface of the contract. Every schema listed here is
 * emitted into generated/contract.schema.json; adding a schema without
 * regenerating fails `check:contract`, which keeps the committed artifact the
 * single rendered source of truth.
 */
export const CONTRACT_SCHEMAS: Record<string, z.ZodType> = {
  Health: HealthSchema,
  Capabilities: CapabilitiesSchema,
  ScanStartRequest: ScanStartRequestSchema,
  ScanStatus: ScanStatusSchema,
  LocalScanReport: LocalScanReportSchema,
  LocalScanNode: LocalScanNodeSchema,
  LocalScanStreamNode: LocalScanStreamNodeSchema,
  LocalScanProgress: LocalScanProgressSchema,
  LocalScanMessage: LocalScanMessageSchema,
  ScanCoverage: ScanCoverageSchema,
  ScanVolume: ScanVolumeSchema,
  TreeSliceRequest: TreeSliceRequestSchema,
  TreeSlice: TreeSliceSchema,
  DiscoveryKind: DiscoveryKindSchema,
  DiscoveryRequest: DiscoveryRequestSchema,
  DiscoveryItem: DiscoveryItemSchema,
  DiscoveryPage: DiscoveryPageSchema,
  AppAuditRequest: AppAuditRequestSchema,
  AppAuditReport: AppAuditReportSchema,
  AppIconRequest: AppIconRequestSchema,
  AppIcon: AppIconSchema,
  RevealPathRequest: RevealPathRequestSchema,
  RevealPathResult: RevealPathResultSchema,
  AppScanOrigin: AppScanOriginSchema,
  AppScanStartRequest: AppScanStartRequestSchema,
  RecentAppScan: RecentAppScanSchema,
  CleanupPlanRequest: CleanupPlanRequestSchema,
  CleanupPlan: CleanupPlanSchema,
  CleanupExecuteRequest: CleanupExecuteRequestSchema,
  CleanupOutcome: CleanupOutcomeSchema,
  AppCleanupPlanRequest: AppCleanupPlanRequestSchema,
  AppCleanupPlan: AppCleanupPlanSchema,
  AppCleanupExecuteRequest: AppCleanupExecuteRequestSchema,
  SessionExchangeRequest: SessionExchangeRequestSchema,
  Session: SessionSchema,
  EventEnvelope: EventEnvelopeSchema,
  Problem: ProblemSchema,
  AiConfigView: AiConfigViewSchema,
  AiChatRequest: AiChatRequestSchema,
  MemoryBreakdown: MemoryBreakdownSchema,
  MemoryPressure: MemoryPressureSchema,
  ProcessRow: ProcessRowSchema,
  ProcessGroup: ProcessGroupSchema,
  MemorySnapshot: MemorySnapshotSchema,
  TerminateTarget: TerminateTargetSchema,
  TerminatePlanRequest: TerminatePlanRequestSchema,
  TerminatePlanEntry: TerminatePlanEntrySchema,
  TerminateRefusal: TerminateRefusalSchema,
  TerminatePlan: TerminatePlanSchema,
  TerminateExecuteRequest: TerminateExecuteRequestSchema,
  TerminateResult: TerminateResultSchema,
  TerminateOutcome: TerminateOutcomeSchema,
  CandidateReason: CandidateReasonSchema,
  FreeUpCandidate: FreeUpCandidateSchema,
  FreeUpCandidates: FreeUpCandidatesSchema,
  CandidatesRequest: CandidatesRequestSchema,
  SystemPoint: SystemPointSchema,
  GroupSeries: GroupSeriesSchema,
  MemoryHistory: MemoryHistorySchema,
  HistoryRequest: HistoryRequestSchema,
  AuditRecord: AuditRecordSchema,
  AuditLog: AuditLogSchema,
}

export function buildContractArtifacts(): Record<string, string> {
  const defs: Record<string, unknown> = {}
  for (const [name, schema] of Object.entries(CONTRACT_SCHEMAS)) {
    defs[name] = z.toJSONSchema(schema, { io: 'output', unrepresentable: 'any', cycles: 'ref' })
  }
  const document = {
    $schema: 'http://json-schema.org/draft-07/schema#',
    title: 'space-lens contract',
    apiMajor: API_MAJOR,
    contractVersion: CONTRACT_VERSION,
    $defs: defs,
  }
  return { 'contract.schema.json': `${JSON.stringify(document, null, 2)}\n` }
}
