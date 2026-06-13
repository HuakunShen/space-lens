export type Preset = 'node' | 'rust' | 'gitignored'
export type SortMode = 'size' | 'path'
export type TuiMode = 'scan' | 'clean'

export interface CliOptions {
  paths: string[]
  presets: Preset[]
  ignoreHidden: boolean
  sort: SortMode
}

export interface PlanEntry {
  path: string
  size: number
  preset: string
  reason: string
}

export interface PlanLike {
  entries: PlanEntry[]
  totalSize: number
  errors: string[]
}

export interface ViewRow extends PlanEntry {
  index: number
  sizeLabel: string
  selected: boolean
  active: boolean
}

export interface DirectoryNode {
  name: string
  path: string
  size: number
  children: DirectoryNode[]
  depth: number
  ignored: boolean
  collapsed: boolean
}

export interface TreeRow {
  index: number
  label: string
  path: string
  size: number
  sizeLabel: string
  depth: number
  ignored: boolean
  collapsed: boolean
  active: boolean
}

export interface CleanViewModel {
  title: string
  summary: string
  rows: ViewRow[]
  cursor: number
  errors: string[]
}

export interface ScanViewModel {
  title: string
  summary: string
  rows: TreeRow[]
  cursor: number
}

export interface TuiState {
  mode: TuiMode
  scanCursor: number
  cleanCursor: number
  selectedPaths: Set<string>
  confirmExecute: boolean
  status: string
}

export type TuiAction =
  | { type: 'switch-mode' }
  | { type: 'move'; delta: number; rowCount: number }
  | { type: 'toggle-selected'; path: string }
  | { type: 'toggle-all'; paths: string[] }
  | { type: 'request-execute' }
  | { type: 'cancel-execute' }
  | { type: 'set-status'; status: string }
  | { type: 'clear-selection' }

export function createInitialTuiState(): TuiState {
  return {
    mode: 'scan',
    scanCursor: 0,
    cleanCursor: 0,
    selectedPaths: new Set(),
    confirmExecute: false,
    status: '',
  }
}

export function createCleanViewModel(
  plan: PlanLike,
  options: Pick<CliOptions, 'sort'> & { state: TuiState },
): CleanViewModel {
  const rows = [...plan.entries].sort(compareEntries(options.sort)).map((entry, index) => ({
    ...entry,
    index: index + 1,
    sizeLabel: formatBytes(entry.size),
    selected: options.state.selectedPaths.has(entry.path),
    active: index === clampCursor(options.state.cleanCursor, plan.entries.length),
  }))

  const noun = rows.length === 1 ? 'candidate' : 'candidates'
  const selectedSize = rows.filter((row) => row.selected).reduce((total, row) => total + row.size, 0)

  return {
    title: 'Space Lens',
    summary: `${rows.length} ${noun} | ${formatBytes(plan.totalSize)} | selected ${formatBytes(selectedSize)}`,
    rows,
    cursor: clampCursor(options.state.cleanCursor, rows.length),
    errors: plan.errors,
  }
}

export function createScanViewModel(trees: DirectoryNode[], state: TuiState): ScanViewModel {
  const rows = trees.flatMap((tree) => flattenTree(tree))
  const totalSize = trees.reduce((total, tree) => total + tree.size, 0)
  const cursor = clampCursor(state.scanCursor, rows.length)

  return {
    title: 'Space Lens',
    summary: `${rows.length} ${rows.length === 1 ? 'node' : 'nodes'} | ${formatBytes(totalSize)}`,
    rows: rows.map((row, index) => ({
      ...row,
      index: index + 1,
      active: index === cursor,
    })),
    cursor,
  }
}

export function applyTuiAction(state: TuiState, action: TuiAction): void {
  switch (action.type) {
    case 'switch-mode':
      state.mode = state.mode === 'scan' ? 'clean' : 'scan'
      state.confirmExecute = false
      return
    case 'move':
      if (state.mode === 'scan') {
        state.scanCursor = moveCursor(state.scanCursor, action.delta, action.rowCount)
      } else {
        state.cleanCursor = moveCursor(state.cleanCursor, action.delta, action.rowCount)
      }
      state.confirmExecute = false
      return
    case 'toggle-selected':
      if (state.selectedPaths.has(action.path)) {
        state.selectedPaths.delete(action.path)
      } else {
        state.selectedPaths.add(action.path)
      }
      state.confirmExecute = false
      return
    case 'toggle-all':
      if (action.paths.length === 0) {
        return
      }
      if (action.paths.every((path) => state.selectedPaths.has(path))) {
        for (const path of action.paths) {
          state.selectedPaths.delete(path)
        }
      } else {
        for (const path of action.paths) {
          state.selectedPaths.add(path)
        }
      }
      state.confirmExecute = false
      return
    case 'request-execute':
      state.confirmExecute = true
      return
    case 'cancel-execute':
      state.confirmExecute = false
      return
    case 'set-status':
      state.status = action.status
      return
    case 'clear-selection':
      state.selectedPaths.clear()
      state.confirmExecute = false
      return
  }
}

export function getSelectedEntries(state: Pick<TuiState, 'selectedPaths'>, entries: PlanEntry[]): PlanEntry[] {
  return entries.filter((entry) => state.selectedPaths.has(entry.path))
}

export function formatBytes(input: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB', 'EiB']
  const bytes = Math.max(0, Math.trunc(Number.isFinite(input) ? input : 0))

  if (bytes < 1024) {
    return `${bytes} B`
  }

  let value = bytes
  let unit = 0

  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }

  return `${value.toFixed(1)} ${units[unit]} (${bytes} bytes)`
}

function compareEntries(sort: SortMode) {
  return (left: PlanEntry, right: PlanEntry) => {
    if (sort === 'path') {
      return left.path.localeCompare(right.path)
    }

    return right.size - left.size || left.path.localeCompare(right.path)
  }
}

function flattenTree(node: DirectoryNode, depth = 0): Omit<TreeRow, 'index' | 'active'>[] {
  const suffix = [node.ignored ? '[ignored]' : '', node.collapsed ? '[collapsed]' : ''].filter(Boolean).join(' ')
  const label = `${'  '.repeat(depth)}${node.name}${suffix ? ` ${suffix}` : ''}`

  return [
    {
      label,
      path: node.path,
      size: node.size,
      sizeLabel: formatBytes(node.size),
      depth,
      ignored: node.ignored,
      collapsed: node.collapsed,
    },
    ...node.children.flatMap((child) => flattenTree(child, depth + 1)),
  ]
}

function moveCursor(cursor: number, delta: number, rowCount: number): number {
  if (rowCount <= 0) {
    return 0
  }

  return clampCursor(cursor + delta, rowCount)
}

function clampCursor(cursor: number, rowCount: number): number {
  if (rowCount <= 0) {
    return 0
  }

  return Math.max(0, Math.min(cursor, rowCount - 1))
}
