import type {
  Capabilities,
  ChildrenPage,
  ChildrenPageRequest,
  CleanupExecuteRequest,
  CleanupOutcome,
  CleanupPlan,
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

/**
 * Ports injected by the host app (which owns the @tauri-apps/api import —
 * this module never imports it, so the browser bundle stays clean).
 * `createChannel` hands back a real Tauri Channel wired to `onmessage`.
 */
export interface TauriPorts {
  invoke(cmd: string, payload?: Record<string, unknown>): Promise<unknown>
  createChannel(onmessage: (message: unknown) => void): unknown
}

interface Reply<T> {
  ok: boolean
  result?: T
  problem?: Problem
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

/**
 * The invoke RESPONSE body (custom-protocol fetch) is unreliable on macOS —
 * later responses never resolve (see apps/desktop/README.md). So every reply
 * rides a Tauri **Channel** (event delivery) and the invoke body is ignored:
 * the fetch may fail silently without affecting the call.
 */
function invokeWithReply<T>(ports: TauriPorts, cmd: string, payload: Record<string, unknown>): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${cmd} timed out`)), REPLY_TIMEOUT_MS)
    const channel = ports.createChannel((message) => {
      clearTimeout(timer)
      const reply = message as Reply<T>
      if (reply.ok === true && reply.result === undefined) {
        reject(new Error(`reply.result missing: ${JSON.stringify(message).slice(0, 300)}`))
        return
      }
      if (reply.ok === true) resolve(reply.result as T)
      else if (reply.problem !== undefined) reject(new TauriProblemError(reply.problem))
      else reject(new Error('malformed reply'))
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

  const invokeReply = async <T>(cmd: string, payload: Record<string, unknown>): Promise<T> => {
    if (sessionId === '') {
      const metadata = (await ports.invoke('sl_connect', {})) as { sessionId: string }
      sessionId = metadata.sessionId
    }
    return invokeWithReply<T>(ports, cmd, { sessionId, ...payload })
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
      const { paths, ignoreHidden, respectGitignore, ignoredMode, label } = body
      return invokeReply<ScanSession>('sl_submit', {
        request: { kind: 'scanStart', paths, ignoreHidden, respectGitignore, ignoredMode, label: label ?? null },
      })
    },
    async scanStatus(scanId: string) {
      return invokeReply<ScanStatus>('sl_read', { request: { method: 'scanStatus', scanId } })
    },
    async cancelScan(scanId: string) {
      return invokeReply<ScanStatus>('sl_read', { request: { method: 'scanCancel', scanId } })
    },
    async treeSlice(body: TreeSliceRequest) {
      const { scanId, nodeId, depth, maxChildrenPerNode } = body
      return invokeReply<TreeSlice>('sl_read', { request: { method: 'treeSlice', scanId, nodeId, depth, maxChildrenPerNode } })
    },
    async children(body: ChildrenPageRequest) {
      const { scanId, nodeId, offset, limit, sort } = body
      return invokeReply<ChildrenPage>('sl_read', { request: { method: 'treeChildren', scanId, nodeId, offset, limit, sort } })
    },
    async plan(body: { scanId: string; nodeIds: string[] }) {
      return invokeReply<CleanupPlan>('sl_submit', { request: { kind: 'cleanupPlan', scanId: body.scanId, nodeIds: body.nodeIds } })
    },
    async execute(body: CleanupExecuteRequest) {
      return invokeReply<CleanupOutcome>('sl_submit', { request: { kind: 'cleanupExecute', planId: body.planId, confirm: body.confirm } })
    },
  }
}
