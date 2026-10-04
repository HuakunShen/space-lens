import { lstatSync, realpathSync } from 'node:fs'
import { homedir } from 'node:os'
import { basename, dirname, isAbsolute, resolve, sep } from 'node:path'
import { createHash, randomBytes } from 'node:crypto'
import { createRequire } from 'node:module'
import { Worker } from 'node:worker_threads'

import type {
  ChildrenPage,
  CleanupOutcome,
  CleanupPlan,
  DiscoveryRequest,
  DiscoveryPage,
  ScanSession,
  ScanStartRequest,
  ScanStatus,
  TreeSlice,
  TreeNodeSummary,
  TreeSliceNode,
  LocalScanNode,
  LocalScanReport,
} from '@space-lens/contract'
import type { DirectoryNode } from 'space-lens'

import type { EventRing } from './events.ts'
import { ProblemError, Problems } from './problems.ts'
import type { TrashPort } from './trash.ts'
import { DISCOVERY_BOOTSTRAP, type DiscoveredEntry, type DiscoverySnapshot } from './discovery.ts'
import { resolveLocalScanBin, spawnLocalScan, type LocalScanProcess } from './local-process.ts'
import { protectedDiscovery } from './protected-discovery.ts'

type ScanNode = DirectoryNode & Partial<Pick<LocalScanNode, 'logicalSize' | 'isDirectory' | 'scanState' | 'skipReason'>>

/**
 * The engine is loaded by path and handed to the worker explicitly: eval-mode
 * workers get a plain CommonJS `require`, and resolving `space-lens` relative
 * to this module keeps working identically in vite-node, in the bundled CLI,
 * and in tests.
 */
function resolveEnginePath(): string {
  return createRequire(import.meta.url).resolve('space-lens')
}

const WORKER_BOOTSTRAP = `
;(async () => {
  const { parentPort, workerData } = await import('node:worker_threads')
  try {
    // dynamic import works from both CJS- and ESM-evaluated workers; the napi
    // entry is CommonJS so its exports arrive as the default binding
    const loaded = await import(workerData.enginePath)
    const engine = loaded.default ?? loaded
    const tree = engine.scanDirectory(workerData.request)
    parentPort.postMessage({ type: 'done', tree })
  } catch (error) {
    parentPort.postMessage({ type: 'error', message: error && error.message ? String(error.message) : String(error) })
  }
})()
`

function within(path: string, root: string): boolean {
  return path === root || path.startsWith(root.endsWith(sep) ? root : `${root}${sep}`)
}

function distinctRoots(paths: string[]): string[] {
  return [...new Set(paths)].filter((path, _, unique) => !unique.some((root) => root !== path && within(path, root)))
}

/** Known Darwin root aliases are normalized lexically before the CLI guard. */
export function normalizeLocalScanPath(path: string): string {
  const normalized = resolve(path)
  if (process.platform === 'darwin') {
    for (const alias of ['/tmp', '/var', '/etc']) {
      if (normalized === alias || normalized.startsWith(`${alias}/`)) return `/private${normalized}`
    }
  }
  return normalized
}

function nodeIdOf(path: string): string {
  return createHash('sha1').update(path).digest('hex').slice(0, 24)
}

function newId(prefix: string): string {
  return `${prefix}${randomBytes(6).toString('hex')}`
}

interface IndexEntry {
  id: string
  node: ScanNode
  parent: string | null
  childIds: string[]
  depth: number
  fingerprint?: { size: number; mtimeMs: number }
  cleanup?: DiscoveredEntry
}

interface ScanRecord {
  session: ScanSession
  status: ScanStatus
  worker: Worker | null
  tree: DirectoryNode[] | null
  index: Map<string, IndexEntry> | null
  done: Promise<void>
  resolveDone: () => void
  roots: string[]
  discovery: Promise<DiscoverySnapshot> | null
  discoveryWorker: Worker | null
  localOnly: boolean
  gitignoreClassified: boolean
  process: LocalScanProcess | null
}

interface DiscoveryJob {
  record: ScanRecord
  index: Map<string, IndexEntry>
  resolve: (snapshot: DiscoverySnapshot) => void
  reject: (error: Error) => void
}

export interface ScanManagerOptions {
  events: EventRing
  trash: TrashPort
  roots: readonly string[]
  cleanupMode: 'none' | 'trash'
  maxConcurrent: number
  maxTotal: number
  planTtlMs?: number
  /** Absolute protected release CLI path; otherwise env/repository discovery is used. */
  localScanBin?: string
}

export class ScanManager {
  /** Insertion-ordered map doubles as an LRU. */
  private readonly scans = new Map<string, ScanRecord>()
  private readonly plans = new Map<string, CleanupPlan>()
  private readonly options: ScanManagerOptions
  private readonly discoveryQueue: DiscoveryJob[] = []
  private readonly activeDiscovery = new Set<Promise<DiscoverySnapshot>>()
  private readonly activeScanWorkers = new Set<Worker>()
  private readonly activeLocalProcesses = new Set<LocalScanProcess>()
  private closed = false

  constructor(options: ScanManagerOptions) {
    this.options = options
  }

  list(): ScanStatus[] {
    this.sweep()
    return [...this.scans.values()].map((record) => record.status)
  }

  status(scanId: string): ScanStatus {
    return this.requireScan(scanId).status
  }

  async start(request: ScanStartRequest, label: string | undefined): Promise<ScanSession> {
    if (this.closed) throw Problems.unavailable('the host is stopping')
    this.sweep()
    const requestedPaths = request.paths.map((path) =>
      request.localOnly ? this.containLocalRoot(path) : this.containRoot(path),
    )
    // Explicit nested roots may represent separate macOS volumes. Only the
    // protected engine can decide their mount boundaries; preserve them here.
    const resolved = request.localOnly ? [...new Set(requestedPaths)] : distinctRoots(requestedPaths)
    const scanning = this.activeScanWorkers.size + this.activeLocalProcesses.size
    if (scanning + this.activeDiscovery.size >= this.options.maxConcurrent) {
      throw Problems.limit(`at most ${this.options.maxConcurrent} scans or discovery inspections may run at once`)
    }
    if (this.scans.size >= this.options.maxTotal) {
      throw Problems.limit(`at most ${this.options.maxTotal} scan sessions are kept; they expire with the server`)
    }
    const binary = request.localOnly ? resolveLocalScanBin(this.options.localScanBin) : null

    const scanId = newId('scan_')
    const session: ScanSession = {
      scanId,
      rootIds: [],
      createdAt: new Date().toISOString(),
      label: label ?? resolved[0],
    }
    const status: ScanStatus = {
      scanId,
      state: 'scanning',
      message: '',
      progress: null,
      currentPath: null,
      bytesScanned: 0,
      entriesScanned: 0,
      rootIds: [],
      label: session.label,
      updatedAt: new Date().toISOString(),
    }
    let resolveDone: () => void = () => {}
    const done = new Promise<void>((resolve) => {
      resolveDone = resolve
    })
    const record: ScanRecord = {
      session,
      status,
      worker: null,
      tree: null,
      index: null,
      done,
      resolveDone,
      roots: resolved,
      discovery: null,
      discoveryWorker: null,
      localOnly: request.localOnly === true,
      gitignoreClassified: request.respectGitignore,
      process: null,
    }
    this.scans.set(scanId, record)
    this.options.events.publish({ kind: 'scan.updated', status: { ...status } })

    if (binary !== null) {
      this.startLocalProcess(record, binary, request)
      return session
    }

    const worker = new Worker(WORKER_BOOTSTRAP, {
      eval: true,
      workerData: {
        enginePath: resolveEnginePath(),
        request: {
          directories: resolved,
          ignoreHidden: request.ignoreHidden,
          fullPath: true,
          respectGitignore: request.respectGitignore,
          ignoredMode: request.ignoredMode,
          followSymlinks: false,
        },
      },
    })
    record.worker = worker
    this.activeScanWorkers.add(worker)
    worker.on('message', (message: { type: string; tree?: DirectoryNode[]; message?: string }) => {
      if (record.status.state !== 'scanning') return
      if (message.type === 'done' && message.tree) {
        record.tree = message.tree
        record.index = this.buildIndex(message.tree)
        record.status = {
          ...record.status,
          state: 'ready',
          bytesScanned: message.tree.reduce((total, root) => total + root.size, 0),
          entriesScanned: this.countNodes(message.tree),
          rootIds: message.tree.map((root) => nodeIdOf(root.path)),
          updatedAt: new Date().toISOString(),
        }
        record.session = { ...record.session, rootIds: record.status.rootIds }
      } else if (message.type === 'error') {
        record.status = {
          ...record.status,
          state: 'failed',
          message: message.message ?? 'scan failed',
          updatedAt: new Date().toISOString(),
        }
      }
      worker.terminate()
    })
    worker.on('error', (error: Error) => {
      if (record.status.state !== 'scanning') return
      record.status = {
        ...record.status,
        state: 'failed',
        message: String(error.message),
        updatedAt: new Date().toISOString(),
      }
      worker.terminate()
    })
    worker.on('exit', (code) => {
      this.activeScanWorkers.delete(worker)
      if (record.status.state === 'scanning') {
        record.status = {
          ...record.status,
          state: 'failed',
          message: `scan worker exited (${code})`,
          updatedAt: new Date().toISOString(),
        }
      }
      this.finish(record)
    })
    this.options.events.publish({ kind: 'scan.updated', status: { ...record.status } })
    return session
  }

  /** Test hook: resolves when the scan reaches a terminal state. */
  wait(scanId: string): Promise<void> {
    return this.requireScan(scanId).done
  }

  cancel(scanId: string): ScanStatus {
    const record = this.requireScan(scanId)
    if (record.status.state === 'scanning') {
      record.status = {
        ...record.status,
        state: 'cancelled',
        message: 'cancelled by request',
        updatedAt: new Date().toISOString(),
      }
      record.worker?.terminate()
      record.process?.terminate()
      this.options.events.publish({ kind: 'scan.updated', status: { ...record.status } })
    }
    return record.status
  }

  slice(scanId: string, nodeId: string, depth: number, maxChildrenPerNode: number): TreeSlice {
    const { index } = this.requireReady(scanId)
    const entry = index.get(nodeId)
    if (entry === undefined) throw Problems.notFound(`no node ${nodeId} in scan ${scanId}`)
    const ancestors: TreeNodeSummary[] = []
    let cursor = entry
    while (cursor.parent !== null) {
      const parent = index.get(cursor.parent)
      if (parent === undefined) break
      ancestors.unshift(this.summarize(parent))
      cursor = parent
    }
    const tree = this.buildSliceNode(index, entry, depth, maxChildrenPerNode)
    return {
      scanId,
      focusNode: this.summarize(entry),
      ancestors,
      tree,
      totalSize: entry.node.size,
      truncated: tree.truncated,
      omittedBytes: tree.omittedBytes,
      omittedCount: tree.omittedCount,
      generatedAt: new Date().toISOString(),
    }
  }

  children(
    scanId: string,
    nodeId: string,
    offset: number,
    limit: number,
    sort: 'size' | 'name' | 'path',
  ): ChildrenPage {
    const { index } = this.requireReady(scanId)
    const entry = index.get(nodeId)
    if (entry === undefined) throw Problems.notFound(`no node ${nodeId} in scan ${scanId}`)
    const items = entry.childIds
      .map((childId) => this.summarize(index.get(childId)!))
      .sort((a, b) => {
        if (sort === 'size') return b.size - a.size || a.name.localeCompare(b.name)
        if (sort === 'name') return a.name.localeCompare(b.name)
        return a.path.localeCompare(b.path)
      })
    return {
      scanId,
      nodeId,
      items: items.slice(offset, offset + limit),
      offset,
      limit,
      total: items.length,
      sort,
    }
  }

  async discover(request: DiscoveryRequest): Promise<DiscoveryPage> {
    if (this.closed) throw Problems.unavailable('the host is stopping')
    const { record, index } = this.requireReady(request.scanId)
    if (record.localOnly && !record.gitignoreClassified && request.kind === 'gitignored') {
      throw Problems.unsupported(
        'this protected scan did not classify gitignored paths; scan with respectGitignore enabled',
      )
    }
    // One worker and promise per ready scan. Paging and switching kinds never
    // rescan the disk, and native scans never block the server's event loop.
    record.discovery ??= record.localOnly
      ? protectedDiscovery(index.values(), (entry) => this.summarize(entry))
      : this.startDiscovery(record, index)
    const pending = record.discovery
    let snapshot: DiscoverySnapshot
    try {
      snapshot = await pending
    } catch (error) {
      // Failed inspection is retryable; concurrent callers must not clear a
      // newer attempt when they observe the same old rejection.
      if (record.discovery === pending) record.discovery = null
      throw error
    }
    const items = snapshot.items[request.kind]
    // Results are sorted and summed in the worker. Find the threshold by
    // binary search; large lists do not need filtering or summing per page.
    let low = 0
    let high = items.length
    while (low < high) {
      const middle = Math.floor((low + high) / 2)
      if (items[middle].node.size >= request.minSize) low = middle + 1
      else high = middle
    }
    const total = low
    return {
      scanId: request.scanId,
      kind: request.kind,
      items:
        request.offset >= total ? [] : items.slice(request.offset, Math.min(total, request.offset + request.limit)),
      total,
      totalSize: snapshot.cumulativeSizes[request.kind][total],
      offset: request.offset,
      limit: request.limit,
    }
  }

  private startDiscovery(record: ScanRecord, index: Map<string, IndexEntry>): Promise<DiscoverySnapshot> {
    return new Promise((resolve, reject) => {
      this.discoveryQueue.push({ record, index, resolve, reject })
      this.pumpDiscovery()
    })
  }

  private pumpDiscovery(): void {
    const scanning = this.activeScanWorkers.size + this.activeLocalProcesses.size
    while (
      !this.closed &&
      this.discoveryQueue.length > 0 &&
      scanning + this.activeDiscovery.size < this.options.maxConcurrent
    ) {
      const job = this.discoveryQueue.shift()!
      const pending = this.runDiscovery(job.record, job.index)
      this.activeDiscovery.add(pending)
      const release = () => {
        this.activeDiscovery.delete(pending)
        this.pumpDiscovery()
      }
      void pending.then(
        (snapshot) => {
          release()
          job.resolve(snapshot)
        },
        (error: Error) => {
          release()
          job.reject(error)
        },
      )
    }
  }

  private runDiscovery(record: ScanRecord, index: Map<string, IndexEntry>): Promise<DiscoverySnapshot> {
    return new Promise((resolve, reject) => {
      const worker = new Worker(DISCOVERY_BOOTSTRAP, {
        eval: true,
        workerData: { enginePath: resolveEnginePath(), roots: record.roots },
      })
      record.discoveryWorker = worker
      let settled = false
      const finish = async (error: Error | null, snapshot?: DiscoverySnapshot) => {
        if (settled) return
        settled = true
        await worker.terminate()
        record.discoveryWorker = null
        if (error !== null) {
          reject(Problems.unavailable(`discovery failed: ${error.message}`))
          return
        }
        let registered = 0
        for (const entry of snapshot!.entries) {
          if (this.closed || this.scans.get(record.session.scanId) !== record) {
            reject(Problems.cancelled('discovery was stopped'))
            return
          }
          // Keep existing children and sizes exactly as the original scan
          // reported; extra indexed nodes only support discovered selections.
          const original = index.get(entry.id)
          if (original === undefined) index.set(entry.id, entry)
          else original.cleanup = entry
          if (++registered % 1000 === 0) await new Promise<void>((yieldTurn) => setImmediate(yieldTurn))
        }
        resolve(snapshot!)
      }
      worker.on('message', (message: { type: string; result?: DiscoverySnapshot; message?: string }) => {
        if (message.type === 'done' && message.result) finish(null, message.result)
        else finish(new Error(message.message ?? 'worker failed'))
      })
      worker.on('error', (error: Error) => finish(error))
      worker.on('exit', (code) => {
        if (!settled) finish(new Error(`worker exited (${code})`))
      })
    })
  }

  /** Called by the host before closing its listener. */
  async close(): Promise<void> {
    this.closed = true
    const terminations: Promise<number>[] = []
    for (const record of [...this.scans.values()]) {
      if (record.worker !== null) terminations.push(record.worker.terminate())
      if (record.status.state === 'scanning') this.cancel(record.session.scanId)
      record.process?.terminate()
      const terminated = this.discardDiscovery(record)
      if (terminated !== undefined) terminations.push(terminated)
    }
    await Promise.allSettled([
      ...terminations,
      ...this.activeDiscovery,
      ...[...this.activeLocalProcesses].map((process) => process.done),
    ])
  }

  private discardDiscovery(record: ScanRecord): Promise<number> | undefined {
    for (let position = this.discoveryQueue.length - 1; position >= 0; position -= 1) {
      if (this.discoveryQueue[position].record !== record) continue
      const [job] = this.discoveryQueue.splice(position, 1)
      job.reject(Problems.cancelled('discovery was stopped'))
    }
    return record.discoveryWorker?.terminate()
  }

  plan(scanId: string, nodeIds: readonly string[]): CleanupPlan {
    const { record, index } = this.requireReady(scanId)
    if (record.localOnly) throw Problems.unsupported('protected local scans are read-only; cleanup is disabled')
    if (this.options.cleanupMode === 'none') throw Problems.unsupported('this host was started without --allow-cleanup')
    const unknown = nodeIds.filter((nodeId) => !index.has(nodeId))
    if (unknown.length > 0) {
      throw Problems.invalid(`plan references ${unknown.length} unknown node id(s)`, {
        example: unknown[0],
      })
    }
    const selections = [...new Set(nodeIds)].map((nodeId) => {
      const entry = index.get(nodeId)!
      return entry.cleanup ?? entry
    })
    for (const entry of selections) {
      this.guardSelection(record, entry.node.path)
      const current = this.fingerprintOf(entry.node.path)
      if (
        entry.fingerprint === undefined ||
        current.size !== entry.fingerprint.size ||
        current.mtimeMs !== entry.fingerprint.mtimeMs
      ) {
        throw Problems.staleSnapshot(`path changed since scanning: ${entry.node.path}; scan again`)
      }
    }
    const selectedPaths = new Set(selections.map((entry) => entry.node.path))
    const normalized = selections.filter((entry) => {
      let parent = dirname(entry.node.path)
      for (;;) {
        if (selectedPaths.has(parent)) return false
        const next = dirname(parent)
        if (next === parent) return true
        parent = next
      }
    })
    const entries = normalized.map((entry) => {
      return {
        path: entry.node.path,
        size: entry.node.size,
        reason: entry.node.ignored ? 'ignored' : 'staged',
        preset: null,
        ignored: entry.node.ignored,
        fingerprint: this.fingerprintOf(entry.node.path),
      }
    })
    const now = Date.now()
    const plan: CleanupPlan = {
      planId: newId('plan_'),
      scanId,
      mode: 'trash',
      entries,
      totalSize: entries.reduce((total, entry) => total + entry.size, 0),
      errors: [],
      createdAt: new Date(now).toISOString(),
      expiresAt: new Date(now + (this.options.planTtlMs ?? 10 * 60 * 1000)).toISOString(),
    }
    if (this.plans.size >= 64) {
      const oldest = this.plans.keys().next().value
      if (oldest !== undefined) this.plans.delete(oldest)
    }
    this.plans.set(plan.planId, plan)
    return plan
  }

  async execute(planId: string): Promise<CleanupOutcome> {
    const plan = this.plans.get(planId)
    if (plan === undefined) throw Problems.notFound(`no plan ${planId}`)
    this.plans.delete(planId)
    if (Date.parse(plan.expiresAt) <= Date.now()) {
      throw Problems.stalePlan('plan expired; plan again from a fresh scan')
    }
    // Re-verify every fingerprint before touching anything: a directory
    // listing is not proof the content is unchanged. One mismatch fails the
    // whole plan closed.
    const changed: string[] = []
    const { record } = this.requireReady(plan.scanId)
    for (const entry of plan.entries) {
      try {
        this.guardSelection(record, entry.path)
        const current = this.fingerprintOf(entry.path)
        if (current.size !== entry.fingerprint.size || current.mtimeMs !== entry.fingerprint.mtimeMs)
          changed.push(entry.path)
      } catch {
        changed.push(entry.path)
      }
    }
    if (changed.length > 0) {
      throw Problems.stalePlan(`${changed.length} of ${plan.entries.length} entries changed since the plan was made`)
    }

    const outcomes = await this.options.trash.trash(plan.entries.map((entry) => entry.path))
    const trashed: CleanupOutcome['trashed'] = []
    const failed: CleanupOutcome['failed'] = []
    let bytesFreed = 0
    for (const entry of plan.entries) {
      const error = outcomes.get(entry.path)
      if (error === null || error === undefined) {
        trashed.push({ path: entry.path, size: entry.size })
        bytesFreed += entry.size
      } else {
        failed.push({ path: entry.path, code: 'Unavailable', message: error })
      }
    }
    return { trashed, bytesFreed, failed }
  }

  private containRoot(path: string): string {
    // A leading `~` is this process's home, not a literal directory name. The
    // expansion lives here rather than in any one client because every
    // frontend (browser, embed, DSH panel) types paths against the same
    // local machine this server runs on.
    if (path === '~' || path.startsWith('~/') || path.startsWith('~\\')) {
      path = `${homedir()}${path.slice(1)}`
    }
    let resolved: string
    try {
      resolved = realpathSync(path)
    } catch {
      throw Problems.invalid(`path does not exist: ${path}`)
    }
    for (const root of this.options.roots) {
      let resolvedRoot: string
      try {
        resolvedRoot = realpathSync(root)
      } catch {
        continue
      }
      if (within(resolved, resolvedRoot)) return resolved
    }
    throw Problems.forbidden(`path is outside the roots this host serves (${this.options.roots.join(', ')})`)
  }

  /** Node performs no scan-target metadata access outside the CLI protection. */
  private containLocalRoot(path: string): string {
    if (path === '~' || path.startsWith('~/') || path.startsWith('~\\')) path = `${homedir()}${path.slice(1)}`
    if (!isAbsolute(path)) throw Problems.invalid('protected scan paths must be absolute')
    const normalized = normalizeLocalScanPath(path)
    const cloudRoots = [
      resolve(homedir(), 'Library', 'CloudStorage'),
      resolve(homedir(), 'Library', 'Mobile Documents'),
      '/Volumes/GoogleDrive',
    ]
    if (cloudRoots.some((root) => within(normalized, root)))
      throw Problems.forbidden('cloud roots are excluded from protected local scans')
    if (this.options.roots.some((root) => within(normalized, normalizeLocalScanPath(root)))) return normalized
    throw Problems.forbidden('path is outside the roots this host serves')
  }

  private startLocalProcess(record: ScanRecord, binary: string, request: ScanStartRequest): void {
    let lastEventAt = 0
    const process = spawnLocalScan(binary, record.roots, request, (progress) => {
      if (record.status.state !== 'scanning' || this.closed) return
      record.status = {
        ...record.status,
        currentPath: progress.currentPath,
        bytesScanned: progress.bytesScanned,
        entriesScanned: progress.entriesScanned,
        updatedAt: new Date().toISOString(),
      }
      const now = Date.now()
      if (now - lastEventAt >= 250) {
        lastEventAt = now
        this.options.events.publish({ kind: 'scan.updated', status: { ...record.status } })
      }
    })
    record.process = process
    this.activeLocalProcesses.add(process)
    void process.done
      .then((report) => {
        if (record.status.state !== 'scanning' || this.closed) return
        this.acceptLocalReport(record, report)
      })
      .catch((error: unknown) => {
        if (record.status.state !== 'scanning') return
        record.status = {
          ...record.status,
          state: 'failed',
          message: error instanceof Error ? error.message : String(error),
          currentPath: null,
          updatedAt: new Date().toISOString(),
        }
      })
      .finally(() => {
        this.activeLocalProcesses.delete(process)
        record.process = null
        this.finish(record)
      })
  }

  private acceptLocalReport(record: ScanRecord, report: LocalScanReport): void {
    if (process.platform === 'darwin' && report.coverage.protection !== 'macos-no-materialization') {
      throw Problems.unavailable('protected scanner did not establish macOS no-materialization protection')
    }
    const roots = new Set(record.roots)
    if (report.nodes.length !== roots.size || report.nodes.some((node) => !roots.has(node.path))) {
      throw Problems.invalid('protected scanner report does not cover the requested roots')
    }
    const seen = new Set<string>()
    const validate = (node: LocalScanNode, parent: LocalScanNode | null): void => {
      if (
        !isAbsolute(node.path) ||
        resolve(node.path) !== node.path ||
        seen.has(node.path) ||
        !record.roots.some((root) => within(node.path, root)) ||
        (parent !== null && dirname(node.path) !== parent.path) ||
        (node.scanState === 'skipped' && node.children.length > 0)
      ) {
        throw Problems.invalid('protected scanner report contains unsafe or duplicate paths')
      }
      seen.add(node.path)
      for (const child of node.children) validate(child, node)
    }
    for (const root of report.nodes) validate(root, null)
    record.tree = report.nodes
    record.index = this.buildIndex(report.nodes, false)
    record.status = {
      ...record.status,
      state: 'ready',
      message: '',
      currentPath: null,
      bytesScanned: report.nodes.reduce((total, root) => total + root.size, 0),
      entriesScanned: report.coverage.files + report.coverage.directories,
      rootIds: report.nodes.map((root) => nodeIdOf(root.path)),
      coverage: report.coverage,
      volumes: report.volumes,
      updatedAt: new Date().toISOString(),
    }
    record.session = { ...record.session, rootIds: record.status.rootIds }
  }

  private guardSelection(record: ScanRecord, path: string): void {
    if (record.roots.includes(path) || !record.roots.some((root) => within(path, root))) {
      throw Problems.forbidden('scan roots cannot be cleaned up')
    }
    const resolved = this.containRoot(path)
    if (resolved !== path || lstatSync(path).isSymbolicLink()) {
      throw Problems.forbidden('cleanup paths must not follow symlinks')
    }
    if (this.options.roots.some((root) => realpathSync(root) === resolved)) {
      throw Problems.forbidden('configured roots cannot be cleaned up')
    }
  }

  private requireScan(scanId: string): ScanRecord {
    const record = this.scans.get(scanId)
    if (record === undefined) throw Problems.notFound(`no scan ${scanId}`)
    // refresh LRU position
    this.scans.delete(scanId)
    this.scans.set(scanId, record)
    return record
  }

  private requireReady(scanId: string): { record: ScanRecord; index: Map<string, IndexEntry> } {
    const record = this.requireScan(scanId)
    if (record.index === null || record.status.state !== 'ready') {
      throw Problems.staleSnapshot(`scan ${scanId} is not ready (state: ${record.status.state})`)
    }
    return { record, index: record.index }
  }

  private finish(record: ScanRecord): void {
    record.worker = null
    this.options.events.publish(
      record.status.state === 'ready'
        ? { kind: 'scan.completed', status: { ...record.status } }
        : { kind: 'scan.updated', status: { ...record.status } },
    )
    record.resolveDone()
    this.pumpDiscovery()
  }

  private buildIndex(tree: ScanNode[], fingerprintEnabled = true): Map<string, IndexEntry> {
    const index = new Map<string, IndexEntry>()
    const walk = (node: ScanNode, parent: string | null, depth: number): string => {
      const id = nodeIdOf(node.path)
      const childIds = (node.children ?? []).map((child) => walk(child, id, depth + 1))
      let fingerprint: IndexEntry['fingerprint']
      try {
        if (fingerprintEnabled) fingerprint = this.fingerprintOf(node.path)
      } catch {
        /* disappeared during scan; planning fails closed */
      }
      index.set(id, { id, node, parent, childIds, depth, fingerprint })
      return id
    }
    for (const root of tree) walk(root, null, 0)
    return index
  }

  private countNodes(tree: DirectoryNode[]): number {
    let count = 0
    const walk = (node: DirectoryNode): void => {
      count += 1
      for (const child of node.children ?? []) walk(child)
    }
    for (const root of tree) walk(root)
    return count
  }

  private summarize(entry: IndexEntry): TreeNodeSummary {
    const childCount = entry.childIds.length
    return {
      id: entry.id,
      // the engine reports the full path as `name` under fullPath mode; the
      // UI wants the basename
      name: basename(entry.node.path),
      path: entry.node.path,
      size: entry.node.size,
      // the engine's per-node depth is unreliable (hidden roots report 0);
      // the index tracks the real walk depth
      depth: entry.depth,
      ignored: entry.node.ignored,
      collapsed: entry.node.collapsed,
      hasChildren: childCount > 0,
      childCount,
      ...(entry.node.logicalSize === undefined ? {} : { logicalSize: entry.node.logicalSize }),
      ...(entry.node.isDirectory === undefined ? {} : { isDirectory: entry.node.isDirectory }),
      ...(entry.node.scanState === undefined ? {} : { scanState: entry.node.scanState }),
      ...(entry.node.skipReason === undefined ? {} : { skipReason: entry.node.skipReason }),
    }
  }

  private buildSliceNode(
    index: Map<string, IndexEntry>,
    entry: IndexEntry,
    remainingDepth: number,
    maxChildrenPerNode: number,
  ): TreeSliceNode & { truncated: boolean } {
    const summary = this.summarize(entry)
    const children = entry.childIds.map((childId) => index.get(childId)!).sort((a, b) => b.node.size - a.node.size)
    let omittedBytes = 0
    let omittedCount = 0
    const kept = children.slice(0, remainingDepth > 0 ? maxChildrenPerNode : 0)
    const dropped = children.slice(kept.length)
    for (const child of dropped) {
      omittedBytes += child.node.size
      omittedCount += 1
    }
    let truncated = dropped.length > 0
    const sliceChildren: TreeSliceNode[] =
      remainingDepth > 0
        ? kept.map((child) => {
            const slice = this.buildSliceNode(index, child, remainingDepth - 1, maxChildrenPerNode)
            omittedBytes += slice.omittedBytes
            omittedCount += slice.omittedCount
            truncated ||= slice.truncated
            return {
              ...this.summarize(child),
              children: slice.children,
              omittedBytes: slice.omittedBytes,
              omittedCount: slice.omittedCount,
            }
          })
        : []
    return {
      ...summary,
      children: sliceChildren,
      omittedBytes,
      omittedCount,
      truncated,
    }
  }

  private fingerprintOf(path: string): { size: number; mtimeMs: number } {
    try {
      const stats = lstatSync(path)
      return { size: stats.size, mtimeMs: stats.mtimeMs }
    } catch (error) {
      throw Problems.stalePlan(`cannot stat ${path}: ${error instanceof Error ? error.message : String(error)}`)
    }
  }

  private sweep(): void {
    for (const [scanId, record] of [...this.scans]) {
      if (record.status.state === 'scanning') continue
      if (this.scans.size <= this.options.maxTotal) break
      this.scans.delete(scanId)
      this.discardDiscovery(record)
    }
  }
}
