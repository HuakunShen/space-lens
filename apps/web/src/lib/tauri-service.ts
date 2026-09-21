import type { Capabilities, ChildrenPage, ChildrenPageRequest, CleanupOutcome, CleanupPlanRequest, Health, RootsResponse, ScanSession, ScanStartRequest, ScanStatus, TreeSlice, TreeSliceRequest } from '@space-lens/contract'

/**
 * The subset of the transport surface the workbench page consumes. The HTTP
 * service satisfies it structurally; the desktop build binds it to Tauri IPC.
 */
export interface WorkbenchService {
  baseUrl: string
  health(): Promise<Health>
  capabilities(): Promise<Capabilities>
  roots(): Promise<RootsResponse>
  startScan(body: ScanStartRequest): Promise<ScanSession>
  scanStatus(scanId: string): Promise<ScanStatus>
  cancelScan(scanId: string): Promise<ScanStatus>
  treeSlice(body: TreeSliceRequest): Promise<TreeSlice>
  children(body: ChildrenPageRequest): Promise<ChildrenPage>
  plan(body: { scanId: string; nodeIds: string[] }): Promise<{ planId: string }>
  execute(body: { planId: string; confirm: true }): Promise<CleanupOutcome>
}

type Invoke = (cmd: string, payload?: Record<string, unknown>) => Promise<unknown>

function getInvoke(): Invoke {
  const internals = (window as unknown as { __TAURI_INTERNALS__?: { invoke: Invoke } }).__TAURI_INTERNALS__
  if (internals === undefined) throw new Error('not running inside the Tauri shell')
  return internals.invoke.bind(internals)
}

/** Desktop transport: closed sl_* commands over Tauri IPC, no HTTP at all. */
// WKWebView loses concurrent IPC responses intermittently (wry race,
// tauri-apps/wry#1537): serialize every invoke through a promise queue.
let queue: Promise<unknown> = Promise.resolve()

export function createTauriService(): WorkbenchService {
  let cached: Invoke | null = null
  const invoke = (cmd: string, payload?: Record<string, unknown>) => {
    const run = async () => {
      if (cached === null) cached = await getInvoke()
      return cached(cmd, payload)
    }
    const next = queue.then(run, run)
    queue = next.catch(() => {})
    return next
  }
  const read = <T>(request: Record<string, unknown>) => invoke('sl_read', { sessionId: sessionIdCache, request }) as Promise<T>
  const submit = <T>(kind: string, request: Record<string, unknown>) => invoke('sl_submit', { sessionId: sessionIdCache, request: { kind, ...request } }) as Promise<T>

  let sessionIdCache = ''
  const ensureSession = async (): Promise<void> => {
    if (sessionIdCache !== '') return
    const metadata = (await invoke('sl_connect', {})) as { sessionId: string }
    sessionIdCache = metadata.sessionId
  }

  return {
    baseUrl: 'tauri://local',
    async health() {
      await ensureSession()
      return read<Health>({ method: 'health' })
    },
    async capabilities() {
      await ensureSession()
      return read<Capabilities>({ method: 'capabilities' })
    },
    async roots() {
      await ensureSession()
      return read<RootsResponse>({ method: 'roots' })
    },
    async startScan(body: ScanStartRequest) {
      await ensureSession()
      const { paths, ignoreHidden, respectGitignore, ignoredMode, label } = body
      return submit<ScanSession>('scanStart', { paths, ignoreHidden, respectGitignore, ignoredMode, label: label ?? null })
    },
    async scanStatus(scanId: string) {
      await ensureSession()
      return read<ScanStatus>({ method: 'scanStatus', scanId })
    },
    async cancelScan() {
      // the synchronous engine has no in-flight scan to cancel on desktop
      throw new Error('cancel is not needed for the desktop engine')
    },
    async treeSlice(body: TreeSliceRequest) {
      await ensureSession()
      const { scanId, nodeId, depth, maxChildrenPerNode } = body
      return read<TreeSlice>({ method: 'treeSlice', scanId, nodeId, depth, maxChildrenPerNode })
    },
    async children(body: ChildrenPageRequest) {
      await ensureSession()
      const { scanId, nodeId, offset, limit, sort } = body
      return read<ChildrenPage>({ method: 'treeChildren', scanId, nodeId, offset, limit, sort })
    },
    async plan(body: { scanId: string; nodeIds: string[] }) {
      await ensureSession()
      return submit<{ planId: string }>('cleanupPlan', { scanId: body.scanId, nodeIds: body.nodeIds })
    },
    async execute(body: { planId: string; confirm: true }) {
      await ensureSession()
      return submit<CleanupOutcome>('cleanupExecute', { planId: body.planId, confirm: true })
    },
  }
}
