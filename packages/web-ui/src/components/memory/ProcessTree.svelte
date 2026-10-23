<script lang="ts">
  /**
   * The Memory view's process tree: "which process started this one?" (§6.2).
   *
   * The snapshot already carries the parent pid on every process row, so the
   * tree is derived on the client by `lib/memory/tree.ts` — no transport and
   * no Rust change. A row shows the process, the parent it was launched by and
   * the memory it costs; the search keeps the whole ancestor chain of a match
   * visible, which is the answer to "where did this `java` come from?".
   *
   * Props only; the page owns the transport, the selection and the Quit list.
   */
  import type { MemorySnapshot, ProcessGroup } from '@space-lens/contract'
  import { ChevronDown, ChevronRight, Lock, Search } from '@lucide/svelte'
  import { Button } from '../ui/button/index'
  import { Checkbox } from '../ui/checkbox/index'
  import { Input } from '../ui/input/index'
  import { formatBytes, formatPercentValue } from '../../lib/memory/format.ts'
  import { buildForest, flattenForest, searchForest, withAncestors, type TreeRow } from '../../lib/memory/tree.ts'
  import { KIND_LABEL, processKey, protectionReason, type StageTarget } from './ProcessTable.svelte'
  import KindGlyph from './KindGlyph.svelte'
  import ColumnResizer from './ColumnResizer.svelte'
  import { TREE_COLUMNS, loadWidths, saveWidths } from '../../lib/memory/columns.ts'

  interface Props {
    snapshot: MemorySnapshot
    /** The page's shared search text; matching rows keep their ancestors. */
    search?: string
    onSearchChange?: (search: string) => void
    selection: ReadonlySet<string>
    /** Toggle one `proc:` key in the caller's selection. */
    onSelect: (key: string) => void
    /** Open the drawer for the group the row belongs to. */
    onOpen: (group: ProcessGroup) => void
    /** A row was clicked: draw this process in the detail diagram. */
    onFocus?: (pid: number) => void
    /** The process the detail diagram is showing, highlighted in the list. */
    focusPid?: number | null
    /** Add a row to the client Quit list (never executes). */
    onStage?: (key: string, target: StageTarget) => void
    /** 'mine' hides the system processes but keeps them as ancestor context. */
    scope?: 'all' | 'mine'
    onScopeChange?: (scope: 'all' | 'mine') => void
    class?: string
  }

  let {
    snapshot,
    search = '',
    onSearchChange,
    selection,
    onSelect,
    onOpen,
    onFocus,
    focusPid = null,
    onStage,
    scope = 'mine',
    onScopeChange,
    class: className = '',
  }: Props = $props()

  const ROW_HEIGHT = 28
  const OVERSCAN = 10
  const INDENT = 14
  const WIDTHS_KEY = 'spacelens.memory.treeColumns'
  /** The fixed checkbox and lock columns either side of the resizable ones. */
  const EDGE_COLUMNS_WIDTH = 64

  type TreeColumn = keyof typeof TREE_COLUMNS
  let widths = $state(loadWidths(WIDTHS_KEY, TREE_COLUMNS))
  function resizeColumn(column: TreeColumn, width: number): void {
    widths = { ...widths, [column]: width }
  }
  /** Rows are exactly as wide as their columns, so spare width collects at the right edge. */
  let rowWidth = $derived(
    EDGE_COLUMNS_WIDTH + widths.name + widths.launchedBy + widths.cpu + widths.memory + widths.pid + widths.kind,
  )

  let collapsed = $state<Set<number>>(new Set())
  /**
   * Focus follows a process, not a row number: the list re-sorts every
   * refresh, and a row index would land on whatever moved into that slot.
   */
  let focusKey = $state<string | null>(null)
  /** Where focus was last, for when its process exits. */
  let focusFallback = 0
  let scrollTop = $state(0)
  let viewportHeight = $state(480)
  let viewportEl = $state<HTMLDivElement | null>(null)

  let forest = $derived(buildForest(snapshot))

  let ownedPids = $derived.by(() => {
    const owned = new Set<number>()
    for (const process of snapshot.processes) {
      if (process.ownedByCurrentUser) owned.add(process.pid)
    }
    return owned
  })

  let searchInfo = $derived(searchForest(forest, search))

  /**
   * Which processes the tree shows. The search wins: it keeps the matches and
   * the ancestors that lead to them, so the answer stays readable. Without a
   * search, the 'mine' scope keeps the user's own processes plus the system
   * processes they descend from — the context without the noise.
   */
  let visible = $derived.by(() => {
    const needle = search.trim()
    if (needle !== '') {
      const matches =
        scope === 'mine'
          ? new Set([...searchInfo.matchPids].filter((pid) => ownedPids.has(pid)))
          : searchInfo.matchPids
      return { keep: withAncestors(forest, matches), expandKept: true }
    }
    if (scope === 'mine') return { keep: withAncestors(forest, ownedPids), expandKept: false }
    return { keep: null, expandKept: false }
  })

  let rows = $derived(
    flattenForest(forest, { collapsed, keep: visible.keep, expandKept: visible.expandKept }),
  )

  let groupById = $derived.by(() => {
    const map = new Map<string, ProcessGroup>()
    for (const group of snapshot.groups) map.set(group.groupId, group)
    return map
  })

  let matchPids = $derived(searchInfo.matchPids)
  let hiddenCount = $derived(snapshot.processes.length - rows.length)

  let focusIndex = $derived.by(() => {
    if (rows.length === 0) return 0
    const at = focusKey === null ? -1 : rows.findIndex((row) => processKey(row.node.pid, row.node.startTimeUs) === focusKey)
    return at >= 0 ? at : Math.min(focusFallback, rows.length - 1)
  })

  let startIndex = $derived(Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN))
  let endIndex = $derived(
    Math.min(rows.length, Math.ceil((scrollTop + viewportHeight) / ROW_HEIGHT) + OVERSCAN),
  )
  let visibleRows = $derived(rows.slice(startIndex, endIndex))

  function toggleExpand(pid: number): void {
    const next = new Set(collapsed)
    if (next.has(pid)) next.delete(pid)
    else next.add(pid)
    collapsed = next
  }

  /** Every process that has something hanging below it. */
  function branches(): number[] {
    const pids: number[] = []
    for (const node of forest.byPid.values()) if (node.children.length > 0) pids.push(node.pid)
    return pids
  }

  /** Open every branch. */
  function expandAll(): void {
    collapsed = new Set()
  }

  /** Open only the roots: the first level of each branch, nothing deeper. */
  function expandTopLevel(): void {
    const roots = new Set(forest.roots.map((root) => root.pid))
    collapsed = new Set(branches().filter((pid) => !roots.has(pid)))
  }

  /** Close every branch: one row per root. */
  function collapseAll(): void {
    collapsed = new Set(branches())
  }

  function memoryText(footprint: number | null, resident: number | null): string {
    if (footprint !== null && footprint > 0) return formatBytes(footprint)
    if (resident !== null && resident > 0) return `≈ ${formatBytes(resident)}`
    return '—'
  }

  function cpuText(value: number | null): string {
    return value === null ? '—' : `${formatPercentValue(value)}%`
  }

  /** The process that started this one, when it is known to the snapshot. */
  function parentName(row: TreeRow): string | null {
    const parent = row.ancestors.at(-1)
    return parent?.name ?? null
  }

  /** The full chain, e.g. `launchd → Claude → zsh → rustc`. */
  function chainText(row: TreeRow): string {
    return [...row.ancestors.map((node) => node.name), row.node.name].join(' → ')
  }

  /**
   * What a collapsed branch hides: how many processes hang below it and how
   * much memory they hold. htop marks a collapsed subtree the same way, and in
   * a memory view the hidden total is the number people actually want.
   */
  function hiddenLabel(row: TreeRow): string | null {
    if (row.expanded || row.node.descendantCount === 0) return null
    const own = row.node.footprintBytes ?? row.node.residentBytes ?? 0
    const hidden = Math.max(0, row.node.subtreeBytes - own)
    return `+${row.node.descendantCount} · ${formatBytes(hidden)}`
  }

  function focusRow(index: number): void {
    if (rows.length === 0) return
    const next = Math.min(Math.max(index, 0), rows.length - 1)
    const row = rows[next]
    focusKey = row === undefined ? null : processKey(row.node.pid, row.node.startTimeUs)
    focusFallback = next
    const element = viewportEl
    if (element === null) return
    const top = next * ROW_HEIGHT
    const bottom = top + ROW_HEIGHT
    if (top < element.scrollTop) element.scrollTop = top
    else if (bottom > element.scrollTop + element.clientHeight) element.scrollTop = bottom - element.clientHeight
    scrollTop = element.scrollTop
  }

  function stageRow(row: TreeRow): void {
    if (!onStage) return
    const node = row.node
    onStage(processKey(node.pid, node.startTimeUs), {
      kind: 'process',
      pid: node.pid,
      startTimeUs: node.startTimeUs,
      name: node.name,
      groupId: node.groupId,
      footprintBytes: node.footprintBytes,
    })
  }

  function onKeydown(event: KeyboardEvent): void {
    if (rows.length === 0) return
    const row = rows[focusIndex]
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault()
        focusRow(focusIndex + 1)
        break
      case 'ArrowUp':
        event.preventDefault()
        focusRow(focusIndex - 1)
        break
      case 'ArrowRight':
        if (row && row.hasChildren && !row.expanded) {
          event.preventDefault()
          toggleExpand(row.node.pid)
        } else {
          event.preventDefault()
          focusRow(focusIndex + 1)
        }
        break
      case 'ArrowLeft':
        if (row && row.hasChildren && row.expanded) {
          event.preventDefault()
          toggleExpand(row.node.pid)
        } else {
          event.preventDefault()
          focusRow(focusIndex - 1)
        }
        break
      case 'Home':
        event.preventDefault()
        focusRow(0)
        break
      case 'End':
        event.preventDefault()
        focusRow(rows.length - 1)
        break
      case ' ':
      case 'Spacebar':
        if (row) {
          event.preventDefault()
          onSelect(processKey(row.node.pid, row.node.startTimeUs))
        }
        break
      case 'Enter': {
        if (!row) break
        event.preventDefault()
        const group = groupById.get(row.node.groupId)
        if (group) onOpen(group)
        break
      }
      case 'Delete':
        if (row && onStage) {
          event.preventDefault()
          stageRow(row)
        }
        break
    }
  }

  function rowIndexFromEvent(event: MouseEvent): number | null {
    const target = event.target
    if (!(target instanceof HTMLElement)) return null
    const row = target.closest<HTMLElement>('[data-row-index]')
    if (row === null) return null
    const raw = row.dataset.rowIndex
    if (raw === undefined) return null
    const index = Number.parseInt(raw, 10)
    return Number.isNaN(index) ? null : index
  }
</script>

<div class={`memory-tree flex flex-col gap-2 ${className}`}>
  <div class="flex flex-wrap items-center gap-2">
    <div class="relative min-w-52 flex-1">
      <Search class="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground" />
      <Input
        class="pl-7"
        placeholder="Search a process to see what launched it"
        aria-label="Search processes"
        value={search}
        oninput={(event) => onSearchChange?.(event.currentTarget.value)}
      />
    </div>

    {#if onScopeChange}
      <div class="flex items-center gap-1" role="group" aria-label="Which processes to show">
        <Button
          variant={scope === 'all' ? 'secondary' : 'ghost'}
          size="xs"
          aria-pressed={scope === 'all'}
          onclick={() => onScopeChange('all')}
        >
          All
        </Button>
        <Button
          variant={scope === 'mine' ? 'secondary' : 'ghost'}
          size="xs"
          aria-pressed={scope === 'mine'}
          onclick={() => onScopeChange('mine')}
        >
          Mine
        </Button>
      </div>
    {/if}

    <div class="ml-auto flex items-center gap-1">
      <Button variant="ghost" size="xs" onclick={expandTopLevel}>Top level</Button>
      <Button variant="ghost" size="xs" onclick={expandAll}>Expand all</Button>
      <Button variant="ghost" size="xs" onclick={collapseAll}>Collapse all</Button>
    </div>
  </div>

  <div
    bind:this={viewportEl}
    bind:clientHeight={viewportHeight}
    class="relative h-[30rem] overflow-auto rounded-md border"
    role="tree"
    aria-label="Process tree"
    tabindex="0"
    onscroll={(event) => (scrollTop = event.currentTarget.scrollTop)}
    onkeydown={onKeydown}
    onclick={(event) => {
      const index = rowIndexFromEvent(event)
      if (index === null) return
      focusRow(index)
      // The checkbox is a control of its own; clicking it must not move the
      // diagram's focus away from the process the user is looking at.
      const row = rows[index]
      const target = event.target
      if (row && !(target instanceof HTMLElement && target.closest('button, input'))) onFocus?.(row.node.pid)
    }}
    ondblclick={(event) => {
      const index = rowIndexFromEvent(event)
      if (index === null) return
      const row = rows[index]
      const group = row ? groupById.get(row.node.groupId) : undefined
      if (group) onOpen(group)
    }}
  >
    <div class="process-table-head sticky top-0 z-10 border-b">
      <div class="flex h-7 items-center text-xs text-muted-foreground" style:min-width={`${rowWidth}px`}>
        <div class="flex w-8 shrink-0 items-center justify-center">
          <span class="sr-only">Select</span>
        </div>
        <div class="relative flex shrink-0 items-center px-2" style:width={`${widths.name}px`}>
          Process{@render resizer('name', 'Process')}
        </div>
        <div class="relative flex shrink-0 items-center px-2" style:width={`${widths.launchedBy}px`}>
          Launched by{@render resizer('launchedBy', 'Launched by')}
        </div>
        <div class="relative flex shrink-0 items-center justify-end px-2" style:width={`${widths.cpu}px`}>
          CPU{@render resizer('cpu', 'CPU')}
        </div>
        <div class="relative flex shrink-0 items-center justify-end px-2" style:width={`${widths.memory}px`}>
          Memory{@render resizer('memory', 'Memory')}
        </div>
        <div class="relative flex shrink-0 items-center justify-end px-2" style:width={`${widths.pid}px`}>
          PID{@render resizer('pid', 'PID')}
        </div>
        <div class="relative shrink-0 px-2" style:width={`${widths.kind}px`}>
          Kind{@render resizer('kind', 'Kind')}
        </div>
        <div class="w-8 shrink-0"></div>
      </div>
    </div>

    <div class="relative" style={`height: ${rows.length * ROW_HEIGHT}px`}>
      {#each visibleRows as row, index (row.node.pid)}
        {@render treeRow(row, startIndex + index)}
      {/each}
    </div>
  </div>

  <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
    {#if rows.length === 0}
      <span>No processes match this view.</span>
    {:else}
      <span>{rows.length} processes shown</span>
    {/if}
    {#if hiddenCount > 0}
      <span>{hiddenCount} hidden</span>
    {/if}
    {#if forest.orphanCount > 0}
      <span>{forest.orphanCount} processes whose parent is not visible</span>
    {/if}
    {#if forest.cycleCount > 0}
      <span>{forest.cycleCount} broken parent links</span>
    {/if}
  </div>
</div>

{#snippet resizer(column: TreeColumn, label: string)}
  <ColumnResizer
    {label}
    width={widths[column]}
    spec={TREE_COLUMNS[column]}
    onResize={(width) => resizeColumn(column, width)}
    onCommit={() => saveWidths(WIDTHS_KEY, widths)}
  />
{/snippet}

{#snippet treeRow(row: TreeRow, index: number)}
  {@const node = row.node}
  {@const key = processKey(node.pid, node.startTimeUs)}
  {@const focused = index === focusIndex}
  {@const selected = selection.has(key)}
  {@const reason = protectionReason(node.protection)}
  {@const parent = parentName(row)}
  {@const matched = matchPids.has(node.pid)}
  {@const hidden = hiddenLabel(row)}
  {@const diagram = node.pid === focusPid}
  <div
    role="treeitem"
    aria-level={row.depth + 1}
    aria-expanded={row.hasChildren ? row.expanded : undefined}
    aria-selected={selected}
    aria-label={`${node.name}, ${node.pid}${parent === null ? '' : `, launched by ${parent}`}`}
    data-focused={focused}
    data-diagram-focus={diagram}
    data-row-index={index}
    class={`absolute left-0 flex w-full cursor-pointer items-center border-l-2 text-sm ${
      diagram ? 'border-primary bg-primary/5' : focused ? 'border-transparent bg-muted/60' : 'border-transparent'
    } ${diagram ? '' : 'hover:bg-muted/40'}`}
    style={`top: ${index * ROW_HEIGHT}px; height: ${ROW_HEIGHT}px; min-width: ${rowWidth}px`}
  >
    <div class="flex w-8 shrink-0 items-center justify-center">
      <Checkbox
        checked={selected}
        aria-label={`Select ${node.name}`}
        onCheckedChange={() => onSelect(key)}
      />
    </div>

    <div
      class="flex min-w-0 shrink-0 items-center"
      style={`width: ${widths.name}px; padding-left: ${row.depth * INDENT}px`}
    >
      {#if row.hasChildren}
        <button
          type="button"
          class="mr-0.5 shrink-0 text-muted-foreground hover:text-foreground"
          aria-expanded={row.expanded}
          aria-label={`${row.expanded ? 'Collapse' : 'Expand'} ${node.name}`}
          onclick={(event) => {
            event.stopPropagation()
            toggleExpand(node.pid)
          }}
        >
          {#if row.expanded}
            <ChevronDown class="size-3.5" />
          {:else}
            <ChevronRight class="size-3.5" />
          {/if}
        </button>
      {:else}
        <span class="mr-0.5 w-3.5 shrink-0"></span>
      {/if}
      <KindGlyph kind={node.kind} />
      <span class={`truncate ${matched ? 'font-semibold' : 'font-medium'}`} title={chainText(row)}>
        {node.name}
      </span>
      {#if hidden}
        <span class="shrink-0 text-xs text-muted-foreground" title="Hidden below this row">{hidden}</span>
      {/if}
      {#if node.restricted}
        <span class="shrink-0 text-xs text-muted-foreground">(hidden)</span>
      {/if}
      {#if reason}
        <Lock class="ml-1 size-3.5 shrink-0 text-muted-foreground" aria-label={reason} title={reason} />
      {/if}
    </div>

    <div
      class="shrink-0 truncate px-2 text-xs text-muted-foreground"
      style:width={`${widths.launchedBy}px`}
      title={chainText(row)}
    >
      {parent ?? '—'}
    </div>
    <div class="shrink-0 truncate px-2 text-right tabular-nums" style:width={`${widths.cpu}px`}>
      {cpuText(node.cpuPercent)}
    </div>
    <div class="shrink-0 truncate px-2 text-right tabular-nums" style:width={`${widths.memory}px`}>
      {memoryText(node.footprintBytes, node.residentBytes)}
    </div>
    <div class="shrink-0 truncate px-2 text-right tabular-nums" style:width={`${widths.pid}px`}>{node.pid}</div>
    <div class="shrink-0 truncate px-2 text-xs" style:width={`${widths.kind}px`}>{KIND_LABEL[node.kind]}</div>
    <div class="flex w-8 shrink-0 items-center justify-center">
      {#if reason}
        <Lock class="size-3.5 text-muted-foreground" aria-label={reason} title={reason} />
      {/if}
    </div>
  </div>
{/snippet}
