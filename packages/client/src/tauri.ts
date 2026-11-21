import type {
  AppAuditReport,
  AppAuditRequest,
  AppCleanupExecuteRequest,
  AppCleanupPlan,
  AppCleanupPlanRequest,
  AppIcon,
  AppIconRequest,
  AppScanStartRequest,
  AuditLog,
  CandidatesRequest,
  Capabilities,
  ChildrenPage,
  ChildrenPageRequest,
  CleanupExecuteRequest,
  CleanupOutcome,
  CleanupPlan,
  DiscoveryPage,
  DiscoveryRequest,
  FreeUpCandidates,
  FsListRequest,
  FsListResponse,
  Health,
  HistoryRequest,
  MemoryHistory,
  MemorySnapshot,
  Problem,
  RevealAction,
  RootsResponse,
  VolumesResponse,
  ScanSession,
  ScanStartRequest,
  ScanStatus,
  TerminateExecuteRequest,
  TerminateOutcome,
  TerminatePlan,
  TerminatePlanRequest,
  TreeSlice,
  TreeSliceRequest,
} from '@space-lens/contract'
import type { WorkbenchService } from './service.ts'
import {
  AuditLogSchema,
  FreeUpCandidatesSchema,
  MemoryHistorySchema,
  MemorySnapshotSchema,
  ScanListResponseSchema,
  TerminateOutcomeSchema,
  TerminatePlanSchema,
} from '@space-lens/contract'

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

/** Session metadata returned by `sl_connect` / `sl_remote_connect`. */
export interface DesktopSession {
  sessionId: string
  serviceInstanceId: string
  /** Scope for the page cache; distinct per device and generation. */
  cacheNamespace: string
  backendLabel: string
  deviceId: string
  deviceLabel: string
  deviceGeneration: number
  /** True when this session is served by a remote SSH host. */
  remote: boolean
}

/** A concrete entry from the user's `~/.ssh/config`, for the picker. */
export interface RemoteSshHost {
  alias: string
  hostName?: string
  user?: string
  port?: number
  source: string
}

export interface RemoteSshListing {
  configPath: string
  hosts: RemoteSshHost[]
  /** False when a limit or unreadable include means aliases may be missing. */
  complete: boolean
  problems: string[]
}

export interface RemoteDevice {
  deviceId: string
  label: string
  kind: 'local' | 'remote'
  platform: string
  serviceInstanceId: string
  generation: number
}

/** One validated destination. The native side rejects anything else. */
export type SshTarget =
  | { kind: 'alias'; alias: string }
  | { kind: 'host'; user: string; host: string; port?: number | null }

export interface RemoteInstallProgress {
  stage:
    | 'probe'
    | 'resolve'
    | 'download'
    | 'verify'
    | 'agent-install'
    | 'host-check'
    | 'serve-start'
    | 'tunnel'
    | 'pair'
    | 'ready'
  code: string
  retryable: boolean
  message: string
}

export interface RemoteDevices {
  list(): Promise<RemoteDevice[]>
  sshHosts(): Promise<RemoteSshListing>
  /** Provisions and connects; progress arrives before the resolved session. */
  connect(
    target: SshTarget,
    onProgress?: (progress: RemoteInstallProgress) => void,
  ): Promise<DesktopSession>
  disconnect(deviceId: string): Promise<void>
}

const REMOTE_CONNECT_TIMEOUT_MS = 30 * 60_000

function problemFromReply(message: unknown, fallback: string): Error {
  const reply = message as Reply<unknown>
  const raw = reply?.problem
  if (raw !== undefined && raw !== null) {
    const problem = 'problem' in raw ? raw.problem : raw
    return new TauriProblemError(problem)
  }
  return new Error(fallback)
}

/**
 * Desktop adapter: closed sl_* commands over Tauri IPC, session bound to the
 * calling window, replies via Channel. `deviceId` selects which session the
 * adapter talks to; `local` is this machine, everything else is a connected
 * SSH host.
 */
export function createTauriService(
  ports: TauriPorts,
  deviceId: string = 'local',
): WorkbenchService & { readonly deviceId: string; session(): Promise<DesktopSession> } {
  let pending: Promise<DesktopSession> | null = null

  /**
   * One session per adapter. A rejected connect is not cached: the next call
   * retries instead of pinning the failure for the lifetime of the page.
   */
  const session = (): Promise<DesktopSession> => {
    if (pending === null) {
      pending = (ports.invoke('sl_connect', { deviceId }) as Promise<DesktopSession>).catch(
        (error: unknown) => {
          pending = null
          throw error
        },
      )
    }
    return pending
  }

  const invokeReply = async <T>(
    cmd: string,
    payload: Record<string, unknown>,
    timeoutMs = REPLY_TIMEOUT_MS,
  ): Promise<T> => {
    const metadata = await session()
    return invokeWithReply<T>(ports, cmd, { sessionId: metadata.sessionId, ...payload }, timeoutMs)
  }

  return {
    baseUrl: 'tauri://local',
    deviceId,
    session,
    async health() {
      return invokeReply<Health>('sl_read', { request: { method: 'health' } })
    },
    async capabilities() {
      return invokeReply<Capabilities>('sl_read', { request: { method: 'capabilities' } })
    },
    async roots() {
      return invokeReply<RootsResponse>('sl_read', { request: { method: 'roots' } })
    },
    async volumes() {
      return invokeReply<VolumesResponse>('sl_read', { request: { method: 'volumes' } })
    },
    /**
     * Remote sessions only. A local desktop session lists nothing through
     * this arm: it keeps the native folder dialog (`pickFolder`), and the
     * page gates the Browse button on the active device for that reason.
     */
    async listFolder(body: FsListRequest) {
      return invokeReply<FsListResponse>('sl_read', {
        request: { method: 'fsList', ...(body.path === undefined ? {} : { path: body.path }) },
      })
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
    async startAppScan(body: AppScanStartRequest) {
      return invokeReply<ScanSession>(
        'sl_submit',
        { request: { kind: 'appScanStart', auditId: body.auditId, appId: body.appId } },
        DISCOVERY_REPLY_TIMEOUT_MS,
      )
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
    async appAudit(body: AppAuditRequest) {
      const { minSize, includeHeuristic, limit } = body
      return invokeReply<AppAuditReport>('sl_read',
        { request: { method: 'appAudit', minSize, includeHeuristic, limit } },
        DISCOVERY_REPLY_TIMEOUT_MS)
    },
    async appIcon(body: AppIconRequest) {
      return invokeReply<AppIcon>('sl_read',
        { request: { method: 'appIcon', appPath: body.appPath } },
        DISCOVERY_REPLY_TIMEOUT_MS)
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
    async appCleanupPlan(body: AppCleanupPlanRequest) {
      return invokeReply<AppCleanupPlan>('sl_submit', {
        request: { kind: 'appCleanupPlan', auditId: body.auditId, appId: body.appId, paths: body.paths },
      })
    },
    async appCleanupExecute(body: AppCleanupExecuteRequest) {
      return invokeReply<CleanupOutcome>('sl_submit', {
        request: { kind: 'appCleanupExecute', planId: body.planId, confirm: body.confirm },
      })
    },
    async revealAppLocation(body: { appId: string; path: string }) {
      return invokeReply<{ revealed: boolean }>('sl_host_request', {
        request: { kind: 'revealPath', appId: body.appId, path: body.path },
      })
    },
    async revealPath(body: { path: string; action?: RevealAction }) {
      return invokeReply<{ revealed: boolean }>('sl_host_request', {
        request: { kind: 'revealScanPath', path: body.path, action: body.action ?? 'reveal' },
      })
    },
    async pickFolder(title?: string) {
      const result = await invokeReply<{ picked: string | null }>('sl_host_request', {
        request: { kind: 'pickDirectory', title: title ?? null },
      })
      return result.picked
    },
    // Memory view and process control (§4.7/§4.8): closed read/submit/host arms
    // exactly as `apps/desktop/src-tauri/src/commands.rs` deserializes them.
    // Every reply is re-validated against the contract schema, so a drifted
    // native reply fails here instead of in the page. Execute runs real signals,
    // so it keeps the default 60 s reply timeout rather than a shorter one.
    async memorySnapshot() {
      return MemorySnapshotSchema.parse(
        await invokeReply<unknown>('sl_read', { request: { method: 'memorySnapshot' } }),
      )
    },
    async memoryHistory(body: HistoryRequest) {
      return MemoryHistorySchema.parse(
        await invokeReply<unknown>('sl_read', {
          request: { method: 'memoryHistory', groupIds: body.groupIds },
        }),
      )
    },
    async memoryCandidates(body: CandidatesRequest) {
      return FreeUpCandidatesSchema.parse(
        await invokeReply<unknown>('sl_read', {
          request: { method: 'memoryCandidates', exclude: body.exclude },
        }),
      )
    },
    async memoryAudit() {
      return AuditLogSchema.parse(await invokeReply<unknown>('sl_read', { request: { method: 'memoryAudit' } }))
    },
    async memoryTerminatePlan(body: TerminatePlanRequest) {
      const { targets, mode, origin } = body
      return TerminatePlanSchema.parse(
        await invokeReply<unknown>('sl_submit', {
          request: { kind: 'memoryTerminatePlan', targets, mode, origin },
        }),
      )
    },
    async memoryTerminateExecute(body: TerminateExecuteRequest) {
      return TerminateOutcomeSchema.parse(
        await invokeReply<unknown>('sl_submit', {
          request: { kind: 'memoryTerminateExecute', planId: body.planId, confirm: body.confirm },
        }),
      )
    },
    async reopenApp(body: { auditId: string }) {
      return invokeReply<{ reopened: boolean }>('sl_host_request', {
        request: { kind: 'reopenApp', auditId: body.auditId },
      })
    },
  }
}


/**
 * Device management for the desktop shell. Kept outside `WorkbenchService`:
 * connecting over SSH is a host concern, not part of the scan/tree contract
 * every adapter must implement.
 */
export function createTauriRemotes(ports: TauriPorts): RemoteDevices {
  return {
    async list() {
      const reply = (await ports.invoke('sl_remote_devices', {})) as { devices: RemoteDevice[] }
      return Array.isArray(reply?.devices) ? reply.devices : []
    },
    async sshHosts() {
      return (await ports.invoke('sl_remote_ssh_hosts', {})) as RemoteSshListing
    },
    connect(target, onProgress) {
      return new Promise<DesktopSession>((resolve, reject) => {
        const timer = setTimeout(
          () => reject(new Error('remote provisioning timed out')),
          REMOTE_CONNECT_TIMEOUT_MS,
        )
        const progress = ports.createChannel((message) => {
          const value = message as { kind?: string } & RemoteInstallProgress
          if (value?.kind === 'progress') onProgress?.(value)
        })
        const reply = ports.createChannel((message) => {
          clearTimeout(timer)
          const value = message as Reply<DesktopSession>
          if (value?.ok === true && value.result !== undefined) resolve(value.result)
          else reject(problemFromReply(message, 'remote provisioning failed'))
        })
        ports
          .invoke('sl_remote_connect', { target, progress, reply })
          .catch((error: unknown) => {
            clearTimeout(timer)
            reject(error instanceof Error ? error : new Error(String(error)))
          })
      })
    },
    async disconnect(deviceId: string) {
      await ports.invoke('sl_remote_disconnect', { deviceId })
    },
  }
}
