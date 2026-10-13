import type {
  AppAuditRequest,
  AppAuditReport,
  AppCleanupExecuteRequest,
  AppCleanupPlan,
  AppCleanupPlanRequest,
  AppIconRequest,
  AppIcon,
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
}
