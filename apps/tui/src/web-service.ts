import { executeCleanupEntries, loadSpaceLensDataWithProgress } from './scanner.js'
import type { DirectoryNode } from './model.js'
import type { LoadSpaceLensOptions, SpaceLensData } from './scanner.js'
import type { ScanProgressEvent } from 'space-lens'
import type {
  ChildrenPage,
  CleanupOutcome,
  CollectorEntry,
  GetChildrenRequest,
  ScanSession,
  ScanStatus,
  SpaceLensAPI,
  StartScanOptions,
  TreeNodeSummary,
  TreeSlice,
  TreeSliceNode,
} from './web-api.js'

export type LoadData = (
  options: LoadSpaceLensOptions,
  onProgress: (event: ScanProgressEvent) => void,
) => SpaceLensData | Promise<SpaceLensData>

interface IndexedNode extends DirectoryNode {
  id: string
  parentId: string | null
  children: IndexedNode[]
}

interface ServerSession {
  session: ScanSession
  root: IndexedNode | null
  nodes: Map<string, IndexedNode>
  status: ScanStatus
}

export interface SpaceLensServiceOptions {
  loadData?: LoadData
}

export function createSpaceLensAPI(options: SpaceLensServiceOptions = {}): SpaceLensAPI {
  const sessions = new Map<string, ServerSession>()
  const loadData = options.loadData ?? loadSpaceLensDataWithProgress

  return {
    async startScan(scanOptions) {
      const started = createPendingSession(scanOptions)
      sessions.set(started.session.scanId, started)
      void runScan(started, scanOptions, loadData)
      return started.session
    },

    async getNode(request) {
      const session = getReadySession(sessions, request.scanId)
      const node = session.nodes.get(request.nodeId)
      if (!node) throw new WebApiError('Unknown node', 404)
      return createSlice(session, node, request.depth, request.maxChildrenPerNode)
    },

    async getChildren(request) {
      const session = getReadySession(sessions, request.scanId)
      const node = session.nodes.get(request.nodeId)
      if (!node) throw new WebApiError('Unknown node', 404)
      return createChildrenPage(request, node)
    },

    async getScanStatus(scanId) {
      const session = sessions.get(scanId)
      if (!session) throw new WebApiError('Unknown scan', 404)
      return session.status
    },

    async cancelScan(scanId) {
      const session = sessions.get(scanId)
      if (!session) return
      session.status = {
        scanId: session.session.scanId,
        state: 'cancelled',
        message: 'Cancelled',
        progress: null,
        currentPath: session.status.currentPath,
        bytesScanned: session.status.bytesScanned,
        entriesScanned: session.status.entriesScanned,
        rootIds: session.status.rootIds,
        label: session.status.label,
        updatedAt: new Date().toISOString(),
      }
    },

    async planCleanup(cleanupOptions) {
      return planFromCollector(cleanupOptions.entries)
    },

    async executeCleanup(cleanupOptions) {
      const outcome: CleanupOutcome = executeCleanupEntries(planFromCollector(cleanupOptions.entries).entries)
      return outcome
    },

    async showInFileManager() {
      throw new WebApiError('Host file manager integration is not wired yet.', 501)
    },

    async openInTerminal() {
      throw new WebApiError('Host terminal integration is not wired yet.', 501)
    },
  }
}

export class WebApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message)
  }
}

export function statusForError(error: unknown): 400 | 404 | 409 | 500 | 501 {
  if (error instanceof WebApiError) return error.status as 400 | 404 | 409 | 500 | 501
  return 500
}

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function getReadySession(sessions: Map<string, ServerSession>, scanId: string): ServerSession {
  const session = sessions.get(scanId)
  if (!session) throw new WebApiError('Unknown scan', 404)
  if (session.status.state !== 'ready') throw new WebApiError('Scan is still running', 409)
  return session
}

function createPendingSession(options: StartScanOptions): ServerSession {
  const scanId = `scan-${Date.now().toString(36)}`
  const session: ScanSession = {
    scanId,
    rootIds: [],
    createdAt: new Date().toISOString(),
    label: labelFromPaths(options.paths),
  }
  return {
    session,
    root: null,
    nodes: new Map<string, IndexedNode>(),
    status: {
      scanId,
      state: 'scanning',
      message: 'Building storage map...',
      progress: null,
      currentPath: null,
      bytesScanned: 0,
      entriesScanned: 0,
      rootIds: [],
      label: session.label,
      updatedAt: session.createdAt,
    },
  }
}

async function runScan(session: ServerSession, options: StartScanOptions, loadData: LoadData): Promise<void> {
  const updateProgress = createProgressSampler(session)

  try {
    const data = await loadData(
      {
        paths: options.paths,
        presets: ['node', 'rust', 'gitignored'],
        ignoreHidden: options.ignoreHidden,
        respectGitignore: options.respectGitignore,
        ignoredMode: options.ignoredMode,
      },
      updateProgress,
    )

    if (session.status.state === 'cancelled') return

    const roots = data.scanTrees.map((node, index) => indexNode(node, `${index}`, null))
    const root = roots.length === 1 ? roots[0] : createSyntheticRoot(roots)
    const nodes = new Map<string, IndexedNode>()
    collectNodes(root, nodes)
    session.root = root
    session.nodes = nodes
    session.session.rootIds = [root.id]
    session.session.label = root.name
    session.status = {
      ...session.status,
      state: 'ready',
      message: 'Scan ready',
      progress: 1,
      currentPath: root.path,
      bytesScanned: root.size,
      entriesScanned: nodes.size,
      rootIds: [root.id],
      label: root.name,
      updatedAt: new Date().toISOString(),
    }
  } catch (cause) {
    session.status = {
      ...session.status,
      state: 'failed',
      message: cause instanceof Error ? cause.message : 'Scan failed',
      progress: null,
      updatedAt: new Date().toISOString(),
    }
  }
}

function createProgressSampler(session: ServerSession): (event: ScanProgressEvent) => void {
  let lastUpdate = 0

  return (event) => {
    if (session.status.state !== 'scanning') return
    const now = Date.now()
    if (lastUpdate !== 0 && now - lastUpdate < 200) return
    lastUpdate = now
    session.status = {
      ...session.status,
      state: 'scanning',
      message: `Scanning ${event.path}`,
      currentPath: event.path,
      bytesScanned: event.bytesScanned,
      entriesScanned: event.entriesScanned,
      updatedAt: new Date(now).toISOString(),
    }
  }
}

function labelFromPaths(paths: string[]): string {
  if (paths.length === 1) return paths[0] || 'Selected Folder'
  return `${paths.length} selected folders`
}

function indexNode(node: DirectoryNode, id: string, parentId: string | null): IndexedNode {
  const indexed: IndexedNode = {
    ...node,
    id,
    parentId,
    children: [],
  }
  indexed.children = node.children.map((child, index) => indexNode(child, `${id}-${index}`, id))
  return indexed
}

function createSyntheticRoot(children: IndexedNode[]): IndexedNode {
  const root: IndexedNode = {
    id: 'root',
    parentId: null,
    name: 'Selected Folders',
    path: 'selected://folders',
    size: sum(children),
    children,
    depth: 0,
    ignored: false,
    collapsed: false,
  }
  for (const child of children) {
    child.parentId = root.id
  }
  return root
}

function collectNodes(node: IndexedNode, nodes: Map<string, IndexedNode>): void {
  nodes.set(node.id, node)
  for (const child of node.children) collectNodes(child, nodes)
}

function createSlice(session: ServerSession, focus: IndexedNode, depth: number, maxChildren: number): TreeSlice {
  const tree = sliceNode(focus, depth, maxChildren)
  const children = sortedChildren(focus).slice(0, maxChildren).map(toSummary)
  const omitted = sortedChildren(focus).slice(maxChildren)
  return {
    scanId: session.session.scanId,
    focusNode: toSummary(focus),
    ancestors: ancestorsOf(session, focus).map(toSummary),
    children,
    tree,
    totalSize: focus.size,
    loadedDepth: depth,
    maxDepth: depth,
    truncated: tree.truncated,
    omittedBytes: sum(omitted),
    omittedCount: omitted.length,
  }
}

function sliceNode(node: IndexedNode, depth: number, maxChildren: number): TreeSliceNode {
  const children =
    depth <= 0
      ? []
      : sortedChildren(node)
          .slice(0, maxChildren)
          .map((child) => sliceNode(child, depth - 1, maxChildren))
  const omitted = depth <= 0 ? node.children : sortedChildren(node).slice(maxChildren)
  return {
    ...toSummary(node),
    loadedDepth: depth,
    truncated: omitted.length > 0 || children.some((child) => child.truncated),
    children,
    omittedBytes: sum(omitted),
    omittedCount: omitted.length,
  }
}

function createChildrenPage(request: GetChildrenRequest, node: IndexedNode): ChildrenPage {
  const sorted = [...node.children].sort((left, right) =>
    request.sort === 'name'
      ? left.name.localeCompare(right.name)
      : right.size - left.size || left.name.localeCompare(right.name),
  )
  return {
    scanId: request.scanId,
    nodeId: request.nodeId,
    items: sorted.slice(request.offset, request.offset + request.limit).map(toSummary),
    offset: request.offset,
    limit: request.limit,
    total: sorted.length,
    sort: request.sort,
  }
}

function ancestorsOf(session: ServerSession, node: IndexedNode): IndexedNode[] {
  const ancestors: IndexedNode[] = []
  let current: IndexedNode | undefined = node
  while (current) {
    ancestors.unshift(current)
    current = current.parentId ? session.nodes.get(current.parentId) : undefined
  }
  return ancestors
}

function toSummary(node: IndexedNode): TreeNodeSummary {
  return {
    id: node.id,
    name: node.name,
    path: node.path,
    size: node.size,
    depth: node.depth,
    ignored: node.ignored,
    hasChildren: node.children.length > 0,
    childCount: node.children.length,
    loadedDepth: 0,
    truncated: false,
  }
}

function sortedChildren(node: IndexedNode): IndexedNode[] {
  return [...node.children].sort((left, right) => right.size - left.size || left.name.localeCompare(right.name))
}

function sum(nodes: Array<{ size: number }>): number {
  return nodes.reduce((total, node) => total + node.size, 0)
}

function planFromCollector(entries: CollectorEntry[]) {
  const planned = entries.map((entry) => ({
    path: entry.path,
    size: entry.size,
    reason: 'Collector',
    preset: 'manual',
  }))
  return {
    entries: planned,
    totalSize: planned.reduce((total, entry) => total + entry.size, 0),
    errors: [],
  }
}
