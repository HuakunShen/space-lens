import type {
  Capabilities,
  ChildrenPage,
  ChildrenPageRequest,
  CleanupExecuteRequest,
  CleanupOutcome,
  CleanupPlan,
  DiscoveryPage,
  DiscoveryRequest,
  Health,
  Problem,
  RootsResponse,
  ScanSession,
  ScanStartRequest,
  ScanStatus,
  TreeSlice,
  TreeSliceRequest,
} from '@space-lens/contract'
import type { WorkbenchService } from './service.ts'
import { ScanListResponseSchema } from '@space-lens/contract'

/**
 * Ports injected by the host app (which owns the @tauri-apps/api import —
 * this module never imports it, so the browser bundle stays clean).
 * `createChannel` hands back a real Tauri Channel wired to `onmessage`.
 */
export interface TauriPorts {
  invoke(cmd: string, payload?: Record<string, unknown>): Promise<unknown>
  createChannel(onmessage: (message: unknown) => void): unknown
  /**
   * Registers a Tauri event listener and resolves with its unlisten function.
   * Optional: only the desktop event push (apps/web desktop-events.ts) needs
   * it; adapters without it fall back to polling.
   */
  listen?(event: string, handler: (event: unknown) => void): Promise<() => void>
  /** The process home directory, for expanding `~/` in hand-entered paths. */
  homeDir?(): Promise<string>
}

interface Reply<T> {
  ok: boolean
  result?: T
  problem?: Problem | { problem: Problem }
}

export class TauriProblemError extends Error {
  readonly code: string
  constructor(problem: Problem) {
    super(problem.message)
    this.name = 'TauriProblemError'
    this.code = problem.code
  }
}

const REPLY_TIMEOUT_MS = 60_000
// Discovery enumerates ignored contents as well as the ordinary scan tree.
const DISCOVERY_REPLY_TIMEOUT_MS = 10 * 60_000

/**
 * The invoke RESPONSE body (custom-protocol fetch) is unreliable on macOS —
 * later responses never resolve (see apps/desktop/README.md). So every reply
 * rides a Tauri **Channel** (event delivery) and the invoke body is ignored:
 * the fetch may fail silently without affecting the call.
 */
function invokeWithReply<T>(
  ports: TauriPorts,
  cmd: string,
  payload: Record<string, unknown>,
  timeoutMs: number,
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${cmd} timed out`)), timeoutMs)
    const channel = ports.createChannel((message) => {
      clearTimeout(timer)
      const reply = message as Reply<T>
      if (reply.ok === true && reply.result === undefined) {
        reject(new Error(`reply.result missing: ${JSON.stringify(message).slice(0, 300)}`))
        return
      }
      if (reply.ok === true) resolve(reply.result as T)
      else if (reply.problem !== undefined) {
        const problem = 'problem' in reply.problem ? reply.problem.problem : reply.problem
        reject(new TauriProblemError(problem))
      } else reject(new Error('malformed reply'))
    })
    ports.invoke(cmd, { ...payload, reply: channel }).catch((error: unknown) => {
      clearTimeout(timer)
      reject(error instanceof Error ? error : new Error(String(error)))
    })
  })
}

/**
 * Desktop adapter: closed sl_* commands over Tauri IPC, session bound to the
 * calling window, replies via Channel.
 */
export function createTauriService(ports: TauriPorts): WorkbenchService {
  let sessionId = ''

  const invokeReply = async <T>(
    cmd: string,
    payload: Record<string, unknown>,
    timeoutMs = REPLY_TIMEOUT_MS,
  ): Promise<T> => {
    if (sessionId === '') {
      const metadata = (await ports.invoke('sl_connect', {})) as { sessionId: string }
      sessionId = metadata.sessionId
    }
    return invokeWithReply<T>(ports, cmd, { sessionId, ...payload }, timeoutMs)
  }

  return {
    baseUrl: 'tauri://local',
    async health() {
      return invokeReply<Health>('sl_read', { request: { method: 'health' } })
    },
    async capabilities() {
      return invokeReply<Capabilities>('sl_read', { request: { method: 'capabilities' } })
    },
    async roots() {
      return invokeReply<RootsResponse>('sl_read', { request: { method: 'roots' } })
    },
    async startScan(body: ScanStartRequest) {
      const { paths, ignoreHidden, respectGitignore, ignoredMode, label, localOnly } = body
      return invokeReply<ScanSession>('sl_submit', {
        request: {
          kind: 'scanStart',
          paths,
          ignoreHidden,
          respectGitignore,
          ignoredMode,
          label: label ?? null,
          ...(localOnly === undefined ? {} : { localOnly }),
        },
      })
    },
    async scanStatus(scanId: string) {
      return invokeReply<ScanStatus>('sl_read', { request: { method: 'scanStatus', scanId } })
    },
    async listScans() {
      const scans = await invokeReply<unknown>('sl_read', { request: { method: 'scanList' } })
      return ScanListResponseSchema.parse({ scans }).scans
    },
    async cancelScan(scanId: string) {
      return invokeReply<ScanStatus>('sl_read', { request: { method: 'scanCancel', scanId } })
    },
    async treeSlice(body: TreeSliceRequest) {
      const { scanId, nodeId, depth, maxChildrenPerNode } = body
      return invokeReply<TreeSlice>('sl_read', {
        request: { method: 'treeSlice', scanId, nodeId, depth, maxChildrenPerNode },
      })
    },
    async children(body: ChildrenPageRequest) {
      const { scanId, nodeId, offset, limit, sort } = body
      return invokeReply<ChildrenPage>('sl_read', {
        request: { method: 'treeChildren', scanId, nodeId, offset, limit, sort },
      })
    },
    async discover(body: DiscoveryRequest) {
      const { scanId, kind, minSize, offset, limit } = body
      return invokeReply<DiscoveryPage>(
        'sl_read',
        { request: { method: 'discovery', scanId, kind, minSize, offset, limit } },
        DISCOVERY_REPLY_TIMEOUT_MS,
      )
    },
    async plan(body: { scanId: string; nodeIds: string[] }) {
      return invokeReply<CleanupPlan>('sl_submit', {
        request: { kind: 'cleanupPlan', scanId: body.scanId, nodeIds: body.nodeIds },
      })
    },
    async execute(body: CleanupExecuteRequest) {
      return invokeReply<CleanupOutcome>('sl_submit', {
        request: { kind: 'cleanupExecute', planId: body.planId, confirm: body.confirm },
      })
    },
    async pickFolder(title?: string) {
      const result = await invokeReply<{ picked: string | null }>('sl_host_request', {
        request: { kind: 'pickDirectory', title: title ?? null },
      })
      return result.picked
    },
  }
}
