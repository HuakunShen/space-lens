import { Box, ScrollBox, Text, createCliRenderer } from '@opentui/core'

import {
  applyTuiAction,
  createCleanViewModel,
  createInitialTuiState,
  createScanViewModel,
  formatBytes,
  getSelectedEntries,
  type PlanEntry,
  type SortMode,
} from './model.js'
import type { CleanupOutcome, SpaceLensData } from './scanner.js'

export interface TuiOptions {
  initialData: SpaceLensData
  sort: SortMode
  refreshData: () => SpaceLensData
  executeEntries: (entries: PlanEntry[]) => CleanupOutcome
}

export async function runTui(options: TuiOptions): Promise<void> {
  let data = options.initialData
  let executing = false
  const state = createInitialTuiState()
  const renderer = await createCliRenderer({
    exitOnCtrlC: true,
    clearOnShutdown: true,
    screenMode: 'alternate-screen',
  })

  const render = () => {
    clearRoot()

    const scanView = createScanViewModel(data.scanTrees, state)
    const cleanView = createCleanViewModel(data.plan, { sort: options.sort, state })
    const selected = getSelectedEntries(state, data.plan.entries)
    const activeMode = state.mode === 'scan' ? 'SCAN' : 'CLEAN'

    renderer.root.add(
      Box(
        {
          flexDirection: 'column',
          gap: 1,
          padding: 1,
          width: '100%',
          height: '100%',
        },
        Text({
          content: `Space Lens | ${activeMode} | tab switch | j/k move | q quit`,
          fg: '#7dd3fc',
          height: 1,
          truncate: true,
        }),
        Text({
          content:
            state.mode === 'scan'
              ? `${scanView.summary} | tree view`
              : `${cleanView.summary} | space select | a all | x delete`,
          fg: '#d1d5db',
          height: 1,
          truncate: true,
        }),
        Text({
          content: statusLine(selected, state.confirmExecute, executing, state.status),
          fg: state.confirmExecute ? '#fca5a5' : '#9ca3af',
          height: 1,
          truncate: true,
        }),
        state.mode === 'scan'
          ? renderScanPane(scanView.rows)
          : renderCleanPane(cleanView.rows, selected.length, state.confirmExecute),
        ...errorLines(cleanView.errors),
      ),
    )
  }

  const executeSelected = async () => {
    const selected = getSelectedEntries(state, data.plan.entries)
    if (selected.length === 0) {
      applyTuiAction(state, { type: 'set-status', status: 'Nothing selected.' })
      render()
      return
    }

    executing = true
    applyTuiAction(state, { type: 'set-status', status: `Removing ${selected.length} selected paths...` })
    render()

    try {
      const outcome = options.executeEntries(selected)
      data = options.refreshData()
      applyTuiAction(state, { type: 'clear-selection' })
      applyTuiAction(state, {
        type: 'set-status',
        status: removalStatus(outcome),
      })
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      applyTuiAction(state, { type: 'set-status', status: `Delete failed: ${message}` })
    } finally {
      executing = false
      render()
    }
  }

  const handleInput = (sequence: string) => {
    if (sequence === '\u0003') {
      return false
    }
    if (executing) {
      return true
    }

    const scanRows = createScanViewModel(data.scanTrees, state).rows.length
    const cleanRows = createCleanViewModel(data.plan, { sort: options.sort, state }).rows
    const handled = handleSequence(sequence, cleanRows)
    if (!handled) {
      return false
    }

    if (sequence === '\r' || sequence === '\n') {
      if (state.confirmExecute) {
        void executeSelected()
        return true
      }
    }

    if (sequence === 'j' || sequence === '\x1B[B') {
      applyTuiAction(state, {
        type: 'move',
        delta: 1,
        rowCount: state.mode === 'scan' ? scanRows : cleanRows.length,
      })
    } else if (sequence === 'k' || sequence === '\x1B[A') {
      applyTuiAction(state, {
        type: 'move',
        delta: -1,
        rowCount: state.mode === 'scan' ? scanRows : cleanRows.length,
      })
    }

    render()
    return true
  }

  const handleSequence = (sequence: string, cleanRows: PlanEntry[]) => {
    switch (sequence) {
      case 'q':
        renderer.destroy()
        return true
      case '\t':
        applyTuiAction(state, { type: 'switch-mode' })
        return true
      case 'j':
      case 'k':
      case '\x1B[A':
      case '\x1B[B':
        return true
      case ' ':
        if (state.mode === 'clean') {
          const active = cleanRows[state.cleanCursor]
          if (active) {
            applyTuiAction(state, { type: 'toggle-selected', path: active.path })
          }
        }
        return true
      case 'a':
        if (state.mode === 'clean') {
          applyTuiAction(state, { type: 'toggle-all', paths: cleanRows.map((row) => row.path) })
        }
        return true
      case 'x':
        if (state.mode === 'clean') {
          applyTuiAction(state, { type: 'request-execute' })
        }
        return true
      case '\r':
      case '\n':
        return true
      case '\x1B':
        applyTuiAction(state, { type: 'cancel-execute' })
        return true
      default:
        return false
    }
  }

  const clearRoot = () => {
    for (const child of [...renderer.root.getChildren()]) {
      renderer.root.remove(child.id)
    }
  }

  renderer.addInputHandler(handleInput)
  render()
}

function renderScanPane(rows: ReturnType<typeof createScanViewModel>['rows']) {
  return ScrollBox(
    {
      border: true,
      borderStyle: 'rounded',
      borderColor: '#374151',
      flexGrow: 1,
      padding: 1,
      scrollY: true,
      title: 'scan tree',
    },
    ...rows.map((row) =>
      Text({
        content: `${row.active ? '>' : ' '} ${row.sizeLabel.padEnd(24, ' ')} ${row.label}`,
        fg: row.active ? '#bfdbfe' : row.ignored ? '#fde68a' : '#e5e7eb',
        height: 1,
        truncate: true,
      }),
    ),
    ...emptyState(rows.length, 'No scan results.'),
  )
}

function renderCleanPane(
  rows: ReturnType<typeof createCleanViewModel>['rows'],
  selectedCount: number,
  confirmExecute: boolean,
) {
  return ScrollBox(
    {
      border: true,
      borderStyle: 'rounded',
      borderColor: confirmExecute ? '#991b1b' : '#374151',
      flexGrow: 1,
      padding: 1,
      scrollY: true,
      title: selectedCount > 0 ? `cleanup candidates (${selectedCount} selected)` : 'cleanup candidates',
    },
    ...rows.map((row) =>
      Text({
        content: `${row.active ? '>' : ' '} ${row.selected ? '[x]' : '[ ]'} ${row.sizeLabel.padEnd(24, ' ')} ${row.preset.padEnd(10, ' ')} ${row.path}  ${row.reason}`,
        fg: row.active ? '#bfdbfe' : colorForPreset(row.preset),
        height: 1,
        truncate: true,
      }),
    ),
    ...emptyState(rows.length, 'No cleanup candidates found.'),
  )
}

function statusLine(selected: PlanEntry[], confirmExecute: boolean, executing: boolean, status: string): string {
  if (executing) {
    return status
  }
  if (confirmExecute) {
    const size = selected.reduce((total, entry) => total + entry.size, 0)
    return `Confirm delete ${selected.length} paths (${formatBytes(size)}): press enter to execute, esc to cancel.`
  }
  if (status) {
    return status
  }

  return 'Scan mode shows disk tree. Clean mode lets you select candidates and execute deletion.'
}

function removalStatus(outcome: CleanupOutcome): string {
  const base = `Removed ${outcome.removed.length} paths, ${formatBytes(outcome.bytesRemoved)}.`
  if (outcome.errors.length === 0) {
    return base
  }

  return `${base} ${outcome.errors.length} errors.`
}

function colorForPreset(preset: string): string {
  switch (preset) {
    case 'node':
      return '#86efac'
    case 'rust':
      return '#fca5a5'
    case 'gitignored':
      return '#fde68a'
    default:
      return '#e5e7eb'
  }
}

function emptyState(rows: number, message: string) {
  if (rows > 0) {
    return []
  }

  return [
    Text({
      content: message,
      fg: '#9ca3af',
      height: 1,
    }),
  ]
}

function errorLines(errors: string[]) {
  if (errors.length === 0) {
    return []
  }

  return [
    Text({
      content: `Warnings: ${errors.join('; ')}`,
      fg: '#fbbf24',
      height: 1,
      truncate: true,
    }),
  ]
}
