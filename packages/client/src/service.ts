import type {
  AppAuditRequest,
  AppAuditReport,
  AppCleanupExecuteRequest,
  AppCleanupPlan,
  AppCleanupPlanRequest,
  AppIconRequest,
  AppIcon,
  AiChatRequest,
  AiConfigView,
  AppScanStartRequest,
  Capabilities,
  ChildrenPageRequest,
  CleanupExecuteRequest,
  CleanupOutcome,
  Health,
  RootsResponse,
  VolumesResponse,
  ScanSession,
  ScanStartRequest,
  ScanStatus,
  TreeSliceRequest,
  ChildrenPage,
  CleanupPlan,
  DiscoveryRequest,
  DiscoveryPage,
  TreeSlice,
  AuditLog,
  CandidatesRequest,
  FreeUpCandidates,
  HistoryRequest,
  MemoryHistory,
  MemorySnapshot,
  TerminateExecuteRequest,
  TerminateOutcome,
  TerminatePlan,
  TerminatePlanRequest,
} from '@space-lens/contract'

/**
 * The adapter contract every Space Lens frontend talks to — the transport seam
 * (refyard's BackendAdapter, scoped to this product). Implementations:
 *   - createHttpService   (packages/client) — REST + SSE over spacelens serve
 *   - createTauriService  (packages/client, ports injected) — Tauri IPC over
 *     the embedded engine
 * Components and pages only ever see this interface.
 */
export interface WorkbenchService {
  /** Display/origin hint ('tauri://local' for the desktop adapter). */
  baseUrl: string
  health(): Promise<Health>
  capabilities(): Promise<Capabilities>
  roots(): Promise<RootsResponse>
  /** Mounted volumes with capacity, for the pre-scan drive list. Optional:
   * older hosts predate the endpoint and answer 404. */
  volumes?(): Promise<VolumesResponse>
  startScan(body: ScanStartRequest): Promise<ScanSession>
  /** V3 app scan: an app becomes a virtual-root session. Optional: older
   * hosts (and the HTTP host this round) predate the submit arm. */
  startAppScan?(body: AppScanStartRequest): Promise<ScanSession>
  listScans?(): Promise<ScanStatus[]>
  scanStatus(scanId: string): Promise<ScanStatus>
  cancelScan(scanId: string): Promise<ScanStatus>
  treeSlice(body: TreeSliceRequest): Promise<TreeSlice>
  children(body: ChildrenPageRequest): Promise<ChildrenPage>
  discover?(body: DiscoveryRequest): Promise<DiscoveryPage>
  appAudit?(body: AppAuditRequest): Promise<AppAuditReport>
  appIcon?(body: AppIconRequest): Promise<AppIcon>
  plan(body: { scanId: string; nodeIds: string[] }): Promise<CleanupPlan>
  execute(body: CleanupExecuteRequest): Promise<CleanupOutcome>
  /** V2 app-scoped cleanup: stages whole audited locations of one app in one
   * audit generation. Optional: older hosts predate the submit arms. */
  appCleanupPlan?(body: AppCleanupPlanRequest): Promise<AppCleanupPlan>
  appCleanupExecute?(body: AppCleanupExecuteRequest): Promise<CleanupOutcome>
  /** Reveals an allow-listed audited app location in Finder. Optional: older
   * hosts predate the host request. */
  revealAppLocation?(body: { appId: string; path: string }): Promise<{ revealed: boolean }>
  /**
   * Opens the host's own folder picker and answers the one absolute path
   * chosen, or `null` for a cancelled dialog. The affordance is gated by the
   * capabilities' `host.folderPicker` flag — the flag, not this method's
   * presence, decides whether the picker button exists.
   */
  pickFolder?(title?: string): Promise<string | null>
  /**
   * Operator-owned AI advisor over the HTTP host. Optional: the Tauri desktop
   * adapter uses its own BYOK IPC instead (`@space-lens/ai-advisor`), and older
   * hosts predate the routes.
   */
  aiConfig?(): Promise<AiConfigView>
  aiChat?(body: AiChatRequest): Promise<unknown>
  /**
   * Memory view and process control (§4.8). Optional as a group: older hosts
   * predate the routes, and the gate checks the capability *and* the method.
   * Every host decides safety in Rust — these only carry the frozen payloads.
   */
  memorySnapshot?(): Promise<MemorySnapshot>
  memoryHistory?(body: HistoryRequest): Promise<MemoryHistory>
  memoryCandidates?(body: CandidatesRequest): Promise<FreeUpCandidates>
  memoryAudit?(): Promise<AuditLog>
  memoryTerminatePlan?(body: TerminatePlanRequest): Promise<TerminatePlan>
  memoryTerminateExecute?(body: TerminateExecuteRequest): Promise<TerminateOutcome>
  /**
   * Reopens an app quit earlier, resolved from an audit record's bundle id.
   * Desktop-only: the HTTP host answers nothing here (the `reopen` capability
   * flag is false for `serve`), so the method exists only on the Tauri adapter.
   */
  reopenApp?(body: { auditId: string }): Promise<{ reopened: boolean }>
  /**
   * Streams one chat completion through the host. The host sends `stream: true`
   * and forwards the upstream SSE bytes as raw text chunks; `onChunk` receives
   * each decoded piece (chunk boundaries are not line boundaries). Throws the
   * same `ServiceError` as the other methods on a non-2xx response.
   */
  aiChatStream?(body: AiChatRequest, onChunk: (text: string) => void, signal?: AbortSignal): Promise<void>
}
