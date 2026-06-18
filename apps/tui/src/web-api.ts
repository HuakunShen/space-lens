export type IgnoredMode = 'summarize' | 'exclude'
export type SortMode = 'size' | 'name'

export interface StartScanOptions {
  paths: string[]
  ignoreHidden: boolean
  respectGitignore: boolean
  ignoredMode: IgnoredMode
  initialDepth: number
  maxChildrenPerNode: number
}

export interface ScanSession {
  scanId: string
  rootIds: string[]
  createdAt: string
  label: string
}

export interface ScanTarget {
  id: string
  label: string
  path: string
  kind: 'volume' | 'folder' | 'multi-folder'
  description: string
  size: number
  used?: number
}

export interface TreeNodeSummary {
  id: string
  name: string
  path: string
  size: number
  depth: number
  ignored: boolean
  hasChildren: boolean
  childCount: number
  loadedDepth: number
  truncated: boolean
}

export interface TreeSliceNode extends TreeNodeSummary {
  children: TreeSliceNode[]
  omittedBytes: number
  omittedCount: number
}

export interface TreeSlice {
  scanId: string
  focusNode: TreeNodeSummary
  ancestors: TreeNodeSummary[]
  children: TreeNodeSummary[]
  tree: TreeSliceNode
  totalSize: number
  loadedDepth: number
  maxDepth: number
  truncated: boolean
  omittedBytes: number
  omittedCount: number
}

export interface ChildrenPage {
  scanId: string
  nodeId: string
  items: TreeNodeSummary[]
  offset: number
  limit: number
  total: number
  sort: SortMode
}

export interface GetNodeRequest {
  scanId: string
  nodeId: string
  depth: number
  maxChildrenPerNode: number
}

export interface GetChildrenRequest {
  scanId: string
  nodeId: string
  offset: number
  limit: number
  sort: SortMode
}

export interface ScanStatus {
  scanId: string
  state: 'idle' | 'scanning' | 'ready' | 'cancelled' | 'failed'
  message: string
  progress: number | null
  currentPath: string | null
  bytesScanned: number
  entriesScanned: number
  rootIds: string[]
  label: string | null
  updatedAt: string
}

export interface CollectorEntry {
  id: string
  scanId: string
  nodeId: string
  path: string
  name: string
  size: number
  addedAt: string
}

export interface CleanupPlanOptions {
  scanId: string
  entries: CollectorEntry[]
}

export interface ExecuteCleanupOptions {
  scanId: string
  entries: CollectorEntry[]
}

export interface RemovalEntry {
  path: string
  size: number
  reason: string
  preset: string
}

export interface RemovalPlan {
  entries: RemovalEntry[]
  totalSize: number
  errors: string[]
}

export interface CleanupOutcome {
  removed: RemovalEntry[]
  bytesRemoved: number
  errors: string[]
}

export interface SpaceLensAPI {
  getScanTargets(): Promise<ScanTarget[]>
  startScan(options: StartScanOptions): Promise<ScanSession>
  getNode(request: GetNodeRequest): Promise<TreeSlice>
  getChildren(request: GetChildrenRequest): Promise<ChildrenPage>
  getScanStatus(scanId: string): Promise<ScanStatus>
  cancelScan(scanId: string): Promise<void>
  planCleanup(options: CleanupPlanOptions): Promise<RemovalPlan>
  executeCleanup(options: ExecuteCleanupOptions): Promise<CleanupOutcome>
  showInFileManager?(path: string): Promise<void>
  openInTerminal?(path: string): Promise<void>
}
