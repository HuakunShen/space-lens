import type {
  Capabilities,
  ChildrenPageRequest,
  CleanupExecuteRequest,
  CleanupOutcome,
  Health,
  RootsResponse,
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
  startScan(body: ScanStartRequest): Promise<ScanSession>
  listScans?(): Promise<ScanStatus[]>
  scanStatus(scanId: string): Promise<ScanStatus>
  cancelScan(scanId: string): Promise<ScanStatus>
  treeSlice(body: TreeSliceRequest): Promise<TreeSlice>
  children(body: ChildrenPageRequest): Promise<ChildrenPage>
  discover?(body: DiscoveryRequest): Promise<DiscoveryPage>
  plan(body: { scanId: string; nodeIds: string[] }): Promise<CleanupPlan>
  execute(body: CleanupExecuteRequest): Promise<CleanupOutcome>
  /**
   * Opens the host's own folder picker and answers the one absolute path
   * chosen, or `null` for a cancelled dialog. The affordance is gated by the
   * capabilities' `host.folderPicker` flag — the flag, not this method's
   * presence, decides whether the picker button exists.
   */
  pickFolder?(title?: string): Promise<string | null>
}
