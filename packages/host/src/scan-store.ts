import { statSync, realpathSync } from 'node:fs'
import { basename } from 'node:path'
import { createHash, randomBytes } from 'node:crypto'
import { createRequire } from 'node:module'
import { Worker } from 'node:worker_threads'

import type {
  ChildrenPage,
  CleanupOutcome,
  CleanupPlan,
  ScanSession,
  ScanStartRequest,
  ScanStatus,
  TreeSlice,
  TreeNodeSummary,
  TreeSliceNode,
} from '@space-lens/contract'
import type { DirectoryNode } from 'space-lens'

import type { EventRing } from './events.ts'
import { ProblemError, Problems } from './problems.ts'
import type { TrashPort } from './trash.ts'

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

function nodeIdOf(path: string): string {
  return createHash('sha1').update(path).digest('hex').slice(0, 24)
}

function newId(prefix: string): string {
  return `${prefix}${randomBytes(6).toString('hex')}`
}

interface IndexEntry {
  id: string
  node: DirectoryNode
  parent: string | null
  childIds: string[]
  depth: number
}

interface ScanRecord {
  session: ScanSession
  status: ScanStatus
  worker: Worker | null
  tree: DirectoryNode[] | null
  index: Map<string, IndexEntry> | null
  done: Promise<void>
  resolveDone: () => void
}

export interface ScanManagerOptions {
  events: EventRing
  trash: TrashPort
  roots: readonly string[]
  cleanupMode: 'none' | 'trash'
  maxConcurrent: number
  maxTotal: number
  planTtlMs?: number
}

export class ScanManager {
  /** Insertion-ordered map doubles as an LRU. */
  private readonly scans = new Map<string, ScanRecord>()
  private readonly plans = new Map<string, CleanupPlan>()
  private readonly options: ScanManagerOptions

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
    this.sweep()
    const resolved = request.paths.map((path) => this.containRoot(path))
    const scanning = [...this.scans.values()].filter((record) => record.status.state === 'scanning').length
    if (scanning >= this.options.maxConcurrent) {
      throw Problems.limit(`at most ${this.options.maxConcurrent} scans may run at once; cancel one first`)
    }
    if (this.scans.size >= this.options.maxTotal) {
      throw Problems.limit(`at most ${this.options.maxTotal} scan sessions are kept; they expire with the server`)
    }

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
    const record: ScanRecord = { session, status, worker: null, tree: null, index: null, done, resolveDone }
    this.scans.set(scanId, record)
    this.options.events.publish({ kind: 'scan.updated', status: { ...status } })

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
        },
      },
    })
    record.worker = worker
    worker.on('message', (message: { type: string; tree?: DirectoryNode[]; message?: string }) => {
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
      this.finish(record)
    })
    worker.on('error', (error: Error) => {
      record.status = {
        ...record.status,
        state: 'failed',
        message: String(error.message),
        updatedAt: new Date().toISOString(),
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
      this.finish(record)
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

  plan(scanId: string, nodeIds: readonly string[]): CleanupPlan {
    const { index } = this.requireReady(scanId)
    if (this.options.cleanupMode === 'none') throw Problems.unsupported('this host was started without --allow-cleanup')
    const unknown = nodeIds.filter((nodeId) => !index.has(nodeId))
    if (unknown.length > 0) {
      throw Problems.invalid(`plan references ${unknown.length} unknown node id(s)`, {
        example: unknown[0],
      })
    }
    const entries = nodeIds.map((nodeId) => {
      const entry = index.get(nodeId)!
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
    for (const entry of plan.entries) {
      try {
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
      if (resolved === resolvedRoot || resolved.startsWith(`${resolvedRoot}/`)) return resolved
    }
    throw Problems.forbidden(`path is outside the roots this host serves (${this.options.roots.join(', ')})`)
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
  }

  private buildIndex(tree: DirectoryNode[]): Map<string, IndexEntry> {
    const index = new Map<string, IndexEntry>()
    const walk = (node: DirectoryNode, parent: string | null, depth: number): string => {
      const id = nodeIdOf(node.path)
      const childIds = (node.children ?? []).map((child) => walk(child, id, depth + 1))
      index.set(id, { id, node, parent, childIds, depth })
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
      const stats = statSync(path)
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
    }
  }
}
