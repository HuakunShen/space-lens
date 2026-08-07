import { createEffect, createMemo, createSignal, For, Show, type JSX } from 'solid-js'
import {
  AnsiCellSurface,
  clampScroll,
  createTuiSolidRoot,
  Scrollbar,
  StyleTable,
  TerminalDriver,
} from '@uniview/tui-solid'
import { Box, Panel, Text, useInput } from '@uniview/tui-solid'

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

interface SpaceLensAppProps {
  options: TuiOptions
  terminalHeight: () => number
  onQuit: () => void
}

export function runTui(options: TuiOptions): Promise<void> {
  return new Promise((resolve) => {
    const styles = new StyleTable()
    const surface = new AnsiCellSurface({
      write: (chunk) => process.stdout.write(chunk),
      styles,
    })
    const initialSize = {
      width: process.stdout.columns ?? 80,
      height: process.stdout.rows ?? 24,
    }
    const [terminalSize, setTerminalSize] = createSignal(initialSize)
    const root = createTuiSolidRoot({ surface, styles, size: initialSize })
    let stopped = false
    let driverStarted = false

    const quit = () => {
      if (stopped) return
      stopped = true
      root.destroy()
      if (driverStarted) driver.stop()
      resolve()
    }

    const driver = new TerminalDriver({
      input: process.stdin,
      output: process.stdout,
      screen: 'alternate',
      mouse: 'click',
      onEvent: (event) => {
        if (event.type === 'resize') {
          setTerminalSize({ width: event.width, height: event.height })
          root.host.renderer.resize({ width: event.width, height: event.height })
        } else {
          root.dispatchInput(event)
        }
      },
    })

    try {
      driver.start()
      driverStarted = true
      root.render(() => <SpaceLensApp options={options} terminalHeight={() => terminalSize().height} onQuit={quit} />)
      process.stdin.on?.('end', quit)
    } catch (error) {
      try {
        root.destroy()
      } finally {
        if (driverStarted) driver.stop()
      }
      throw error
    }
  })
}

export function SpaceLensApp(props: SpaceLensAppProps): JSX.Element {
  const state = createInitialTuiState()
  const [data, setData] = createSignal(props.options.initialData)
  const [executing, setExecuting] = createSignal(false)
  const [revision, setRevision] = createSignal(0)

  const touch = () => setRevision((value) => value + 1)
  const act = (action: Parameters<typeof applyTuiAction>[1]) => {
    applyTuiAction(state, action)
    touch()
  }
  const scan = createMemo(() => {
    revision()
    return createScanViewModel(data().scanTrees, state)
  })
  const clean = createMemo(() => {
    revision()
    return createCleanViewModel(data().plan, { sort: props.options.sort, state })
  })
  const selected = createMemo(() => {
    revision()
    return getSelectedEntries(state, data().plan.entries)
  })
  const viewportHeight = () => listViewportHeight(props.terminalHeight(), clean().errors.length)
  const activeRows = () => (state.mode === 'scan' ? scan().rows : clean().rows)
  const activeMode = () => {
    revision()
    return state.mode
  }
  const status = () => {
    revision()
    return state.status
  }
  const confirmExecute = () => {
    revision()
    return state.confirmExecute
  }

  const move = (delta: number) => {
    act({ type: 'move', delta, rowCount: activeRows().length })
  }
  const toggleCurrent = () => {
    if (activeMode() !== 'clean') return
    const row = clean().rows[clean().cursor]
    if (row) act({ type: 'toggle-selected', path: row.path })
  }
  const executeSelected = () => {
    const entries = selected()
    if (entries.length === 0) {
      act({ type: 'set-status', status: 'Nothing selected.' })
      return
    }

    setExecuting(true)
    act({ type: 'set-status', status: `Removing ${entries.length} selected paths...` })
    try {
      const outcome = props.options.executeEntries(entries)
      setData(props.options.refreshData())
      act({ type: 'clear-selection' })
      act({ type: 'set-status', status: removalStatus(outcome) })
    } catch (error) {
      act({ type: 'set-status', status: `Delete failed: ${error instanceof Error ? error.message : String(error)}` })
    } finally {
      setExecuting(false)
    }
  }

  useInput((input, key) => {
    if (executing()) return
    if (key.ctrl) return props.onQuit()
    if (key.tab) return act({ type: 'switch-mode' })
    if (key.return) return confirmExecute() ? executeSelected() : undefined
    if (key.escape) return act({ type: 'cancel-execute' })
    if (input === 'q') return props.onQuit()
    if (input === 'j' || key.downArrow) return move(1)
    if (input === 'k' || key.upArrow) return move(-1)
    if (key.pageDown) return move(viewportHeight())
    if (key.pageUp) return move(-viewportHeight())
    if (input === ' ') return toggleCurrent()
    if (input === 'a' && activeMode() === 'clean')
      return act({ type: 'toggle-all', paths: clean().rows.map((row) => row.path) })
    if (input === 'x' && activeMode() === 'clean') return act({ type: 'request-execute' })
  })

  return (
    <Box flexDirection="column" gap={1} padding={1} width="100%" height="100%">
      <Text color="cyan">
        Space Lens | {activeMode() === 'scan' ? 'SCAN' : 'CLEAN'} | tab switch | j/k move | q quit
      </Text>
      <Text color="white">
        {activeMode() === 'scan'
          ? `${scan().summary} | tree view`
          : `${clean().summary} | space select | a all | x delete`}
      </Text>
      <Text color={confirmExecute() ? 'red' : 'gray'}>
        {statusLine(selected(), confirmExecute(), executing(), status())}
      </Text>
      <Show
        when={activeMode() === 'scan'}
        fallback={
          <ListPanel
            title={selected().length > 0 ? `cleanup candidates (${selected().length} selected)` : 'cleanup candidates'}
            rows={clean().rows}
            cursor={clean().cursor}
            height={viewportHeight()}
            danger={confirmExecute()}
            empty="No cleanup candidates found."
            renderRow={(row) => (
              <Text color={row.active ? 'cyan' : colorForPreset(row.preset)}>
                {`${row.active ? '>' : ' '} ${row.selected ? '[x]' : '[ ]'} ${row.sizeLabel.padEnd(24)} ${row.preset.padEnd(10)} ${row.path}  ${row.reason}`}
              </Text>
            )}
          />
        }
      >
        <ListPanel
          title="scan tree"
          rows={scan().rows}
          cursor={scan().cursor}
          height={viewportHeight()}
          empty="No scan results."
          renderRow={(row) => (
            <Text color={row.active ? 'cyan' : row.ignored ? 'yellow' : 'white'}>
              {`${row.active ? '>' : ' '} ${row.sizeLabel.padEnd(24)} ${row.label}`}
            </Text>
          )}
        />
      </Show>
      <For each={clean().errors}>{(error) => <Text color="yellow">Warnings: {error}</Text>}</For>
    </Box>
  )
}

interface ListPanelProps<T> {
  title: string
  rows: readonly T[]
  cursor: number
  height: number
  empty: string
  danger?: boolean
  renderRow: (row: T, index: number) => JSX.Element
}

function ListPanel<T>(props: ListPanelProps<T>): JSX.Element {
  const [scrollTop, setScrollTop] = createSignal(0)
  const maxScroll = () => Math.max(0, props.rows.length - props.height)
  const visible = () => props.rows.slice(scrollTop(), scrollTop() + props.height)

  createEffect(() => {
    const cursor = props.cursor
    const height = props.height
    const max = maxScroll()
    setScrollTop((top) => {
      if (cursor < top) return cursor
      if (cursor >= top + height) return Math.min(max, cursor - height + 1)
      return Math.min(top, max)
    })
  })

  const onWheel = (event: { deltaY: number }) => {
    setScrollTop((top) => clampScroll(top + event.deltaY * 3, props.rows.length, props.height))
  }
  const footer = () =>
    props.rows.length > props.height
      ? `${Math.min(scrollTop() + props.height, props.rows.length)} / ${props.rows.length}`
      : undefined

  return (
    <Panel
      title={props.title}
      footer={footer()}
      borderColor={props.danger ? 'red' : 'gray'}
      flexGrow={1}
      flexShrink={1}
      minHeight={0}
      overflow="hidden"
      onWheel={onWheel}
    >
      <Box flexDirection="row" flexGrow={1} flexShrink={1} minWidth={0} height={props.height}>
        <Box flexDirection="column" flexGrow={1} flexShrink={1} minWidth={0} height={props.height}>
          <Show when={props.rows.length > 0} fallback={<Text color="gray">{props.empty}</Text>}>
            <For each={visible()}>{(row, index) => props.renderRow(row, scrollTop() + index())}</For>
          </Show>
        </Box>
        <Scrollbar total={props.rows.length} height={props.height} value={scrollTop()} />
      </Box>
    </Panel>
  )
}

function listViewportHeight(terminalHeight: number, warningCount: number): number {
  return Math.max(1, terminalHeight - 12 - warningCount * 2)
}

function statusLine(selected: PlanEntry[], confirm: boolean, executing: boolean, status: string): string {
  if (executing) return status
  if (confirm)
    return `Confirm delete ${selected.length} paths (${formatBytes(selected.reduce((total, entry) => total + entry.size, 0))}): enter execute, esc cancel.`
  return status || 'Scan mode shows disk tree. Clean mode selects cleanup candidates.'
}

function removalStatus(outcome: CleanupOutcome): string {
  return `Removed ${outcome.removed.length} paths, ${formatBytes(outcome.bytesRemoved)}.${outcome.errors.length > 0 ? ` ${outcome.errors.length} errors.` : ''}`
}

function colorForPreset(preset: string): string {
  return preset === 'node' ? 'green' : preset === 'rust' ? 'red' : preset === 'gitignored' ? 'yellow' : 'white'
}
