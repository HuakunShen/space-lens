<script lang="ts" module>
  /**
   * Shared trivia for the process table and its drawer: selection keys, kind
   * labels and the plain-language reason a process is protected. Kept in the
   * module context so `ProcessDrawer.svelte` reuses the same strings instead of
   * re-declaring them.
   */
  import type { ProcessGroup, ProcessRow } from '@space-lens/contract'

  /** `ProcessGroup['kind']` — the contract ships the enum as a literal array, not a named type. */
  export type ProcessKind = ProcessGroup['kind']
  /** The non-null half of `ProcessGroup['protection']`. */
  export type Protection = NonNullable<ProcessGroup['protection']>

  /** `group:<groupId>` — the selection key for an application group row. */
  export function groupKey(groupId: string): string {
    return `group:${groupId}`
  }

  /** `proc:<pid>:<startUs>` — the selection key for a member process row. */
  export function processKey(pid: number, startTimeUs: number): string {
    return `proc:${pid}:${startTimeUs}`
  }

  export const KIND_LABEL: Record<ProcessKind, string> = {
    app: 'App',
    helper: 'Helper',
    agent: 'Agent',
    cli: 'CLI',
    system: 'System',
    other: 'Other',
  }

  const PROTECTION_REASON: Record<Protection, string> = {
    kernel: 'The system kernel — never quit.',
    init: "The system's first process — never quit.",
    sessionCritical: 'Critical to your login session; quitting it would end the session.',
    systemComponent: 'A macOS system component; it would relaunch and may disrupt the system.',
    selfOrAncestor: 'Space Lens itself or a process it depends on — quitting it would stop the app.',
    selfChild: 'A process Space Lens started.',
    otherUser: 'Owned by another user; Space Lens cannot quit it.',
    unidentified: 'The process identity could not be verified.',
    zombie: 'This process has already exited.',
  }

  /** The lock-icon tooltip for a protection code, or `null` when unprotected. */
  export function protectionReason(protection: Protection | null): string | null {
    return protection === null ? null : PROTECTION_REASON[protection]
  }

  /** What a table row stages into the Quit list (`onStage`). */
  export type StageTarget =
    | { kind: 'group'; groupId: string; name: string; footprintBytes: number }
    | {
        kind: 'process'
        pid: number
        startTimeUs: number
        name: string
        groupId: string
        footprintBytes: number | null
      }

  export type TableRow =
    | { kind: 'group'; key: string; group: ProcessGroup }
    | { kind: 'process'; key: string; group: ProcessGroup; process: ProcessRow }
</script>

<script lang="ts">
  /**
   * Virtualized process table (§6.2 items 3–4, §6.5, §6.6). Fixed 28 px rows,
   * overscan 10, a `scrollTop`-driven slice: at most ~60 rows are in the DOM no
   * matter how many processes exist. The grid exposes `role="grid"`, `aria-sort`,
   * `aria-rowcount` / `aria-rowindex` and roving arrow-key focus; Space toggles
   * selection, Enter opens the drawer, Delete stages into the Quit list.
   *
   * Sort / filter / search / order-freeze are the pure functions from
   * `lib/memory/table.ts`. Props only; no transport.
   */
  import type { MemorySnapshot } from '@space-lens/contract'
  import {
    AppWindow,
    ArrowDown,
    ArrowUp,
    Bot,
    Boxes,
    ChevronDown,
    ChevronRight,
    Lock,
    Pause,
    Play,
    Search,
    Server,
    Square,
    Terminal,
  } from '@lucide/svelte'
  import { Badge } from '../ui/badge/index'
  import { Button } from '../ui/button/index'
  import { Checkbox } from '../ui/checkbox/index'
  import { Input } from '../ui/input/index'
  import { formatBytes, formatPercentValue } from '../../lib/memory/format.ts'
  import {
    applyOrderFreeze,
    selectGroups,
    type MemoryFilter,
    type MemorySortKey,
    type TableQuery,
  } from '../../lib/memory/table.ts'

  interface Props {
    snapshot: MemorySnapshot
    query: TableQuery
    selection: ReadonlySet<string>
    /** Toggle one `group:` / `proc:` key in the caller's selection. */
    onSelect: (key: string) => void
    /** Open the drawer for a row's group. */
    onOpen: (group: ProcessGroup) => void
    /** Add a row to the client Quit list (never executes). */
    onStage?: (key: string, target: StageTarget) => void
    onQueryChange: (query: TableQuery) => void
    /** Optional interval / pause chrome; hidden when the page does not supply it. */
    intervalMs?: number
    onIntervalChange?: (ms: number) => void
    paused?: boolean
    onTogglePause?: () => void
    class?: string
  }

  let {
    snapshot,
    query,
    selection,
    onSelect,
    onOpen,
    onStage,
    onQueryChange,
    intervalMs,
    onIntervalChange,
    paused = false,
    onTogglePause,
    class: className = '',
  }: Props = $props()

  const ROW_HEIGHT = 28
  const OVERSCAN = 10
  const FILTERS: readonly MemoryFilter[] = ['all', 'apps', 'background', 'mine', 'protected']
  const FILTER_LABELS: Record<MemoryFilter, string> = {
    all: 'All',
    apps: 'Apps',
    background: 'Background',
    mine: 'Mine',
    protected: 'Protected',
  }
  const INTERVALS: readonly number[] = [1000, 2000, 5000]

  let expanded = $state<Set<string>>(new Set())
  let showVirtual = $state(false)
  let frozen = $state(false)
  let prevOrder = $state<string[]>([])
  let focusIndex = $state(0)
  let scrollTop = $state(0)
  let viewportHeight = $state(480)
  let viewportEl = $state<HTMLDivElement | null>(null)

  let ordered = $derived(applyOrderFreeze(prevOrder, selectGroups(snapshot, query), frozen))

  $effect(() => {
    if (!frozen) prevOrder = ordered.map((group) => group.groupId)
  })

  let processByPid = $derived.by(() => {
    const map = new Map<number, ProcessRow>()
    for (const process of snapshot.processes) map.set(process.pid, process)
    return map
  })

  let virtualByGroup = $derived.by(() => {
    const map = new Map<string, number>()
    for (const process of snapshot.processes) {
      if (process.virtualBytes === null) continue
      map.set(process.groupId, (map.get(process.groupId) ?? 0) + process.virtualBytes)
    }
    return map
  })

  let rows = $derived.by((): TableRow[] => {
    const out: TableRow[] = []
    for (const group of ordered) {
      out.push({ kind: 'group', key: groupKey(group.groupId), group })
      if (!expanded.has(group.groupId)) continue
      const members = group.pids
        .map((pid) => processByPid.get(pid))
        .filter((process): process is ProcessRow => process !== undefined)
        .sort(
          (a, b) =>
            (b.footprintBytes ?? b.residentBytes ?? 0) - (a.footprintBytes ?? a.residentBytes ?? 0) ||
            a.pid - b.pid,
        )
      for (const process of members) {
        out.push({ kind: 'process', key: processKey(process.pid, process.startTimeUs), group, process })
      }
    }
    return out
  })

  $effect(() => {
    if (rows.length > 0 && focusIndex >= rows.length) focusIndex = rows.length - 1
  })

  let startIndex = $derived(Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN))
  let endIndex = $derived(
    Math.min(rows.length, Math.ceil((scrollTop + viewportHeight) / ROW_HEIGHT) + OVERSCAN),
  )
  let visible = $derived(rows.slice(startIndex, endIndex))

  let columnCount = $derived(showVirtual ? 9 : 8)
  let restrictedCount = $derived(snapshot.processes.filter((process) => process.restricted).length)
  let showRestrictedNote = $derived(snapshot.notes.includes('restrictedProcesses') && restrictedCount > 0)
  let cpuWarmingUp = $derived(snapshot.notes.includes('cpuWarmingUp'))

  function toggleExpand(groupId: string): void {
    const next = new Set(expanded)
    if (next.has(groupId)) next.delete(groupId)
    else next.add(groupId)
    expanded = next
  }

  function sortAria(key: MemorySortKey): 'ascending' | 'descending' | 'none' {
    if (query.sort !== key) return 'none'
    return query.desc ? 'descending' : 'ascending'
  }

  function sortBy(key: MemorySortKey): void {
    if (query.sort === key) onQueryChange({ ...query, desc: !query.desc })
    else onQueryChange({ ...query, sort: key, desc: key !== 'name' })
  }

  function focusRow(index: number): void {
    if (rows.length === 0) return
    focusIndex = Math.min(Math.max(index, 0), rows.length - 1)
    const element = viewportEl
    if (!element) return
    const top = focusIndex * ROW_HEIGHT
    const bottom = top + ROW_HEIGHT
    if (top < element.scrollTop) element.scrollTop = top
    else if (bottom > element.scrollTop + element.clientHeight) element.scrollTop = bottom - element.clientHeight
    scrollTop = element.scrollTop
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
          onSelect(row.key)
        }
        break
      case 'Enter':
        if (row) {
          event.preventDefault()
          onOpen(row.group)
        }
        break
      case 'Delete':
        if (row && onStage) {
          event.preventDefault()
          stageRow(row)
        }
        break
    }
  }

  function stageRow(row: TableRow): void {
    if (!onStage) return
    if (row.kind === 'group') {
      onStage(row.key, {
        kind: 'group',
        groupId: row.group.groupId,
        name: row.group.name,
        footprintBytes: row.group.footprintBytes,
      })
      return
    }
    onStage(row.key, {
      kind: 'process',
      pid: row.process.pid,
      startTimeUs: row.process.startTimeUs,
      name: row.process.name,
      groupId: row.group.groupId,
      footprintBytes: row.process.footprintBytes,
    })
  }

  /** The `data-row-index` of the row a pointer event landed on, if any. */
  function rowIndexFromEvent(event: MouseEvent): number | null {
    const target = event.target
    if (!(target instanceof HTMLElement)) return null
    const row = target.closest<HTMLElement>('[data-row-index]')
    if (!row) return null
    const raw = row.dataset.rowIndex
    if (raw === undefined) return null
    const index = Number.parseInt(raw, 10)
    return Number.isNaN(index) ? null : index
  }

  /** Click delegation: rows are not individually focusable; the grid owns focus. */
  function handleGridClick(event: MouseEvent): void {
    const index = rowIndexFromEvent(event)
    if (index !== null) focusRow(index)
  }

  function handleGridDblClick(event: MouseEvent): void {
    const index = rowIndexFromEvent(event)
    if (index === null) return
    const row = rows[index]
    if (row) onOpen(row.group)
  }

  /** Keep the order frozen while focus moves between rows; unfreeze only on exit. */
  function handleFocusOut(event: FocusEvent): void {
    const next = event.relatedTarget
    if (next instanceof Node && viewportEl?.contains(next)) return
    frozen = false
  }

  function memoryText(footprint: number | null, resident: number | null): string {
    if (footprint !== null && footprint > 0) return formatBytes(footprint)
    if (resident !== null && resident > 0) return `≈ ${formatBytes(resident)}`
    return '—'
  }

  function cpuText(value: number | null): string {
    return value === null ? '—' : `${formatPercentValue(value)}%`
  }

  function rowKind(row: TableRow): ProcessKind {
    return row.kind === 'group' ? row.group.kind : row.process.kind
  }

  function rowProtection(row: TableRow): Protection | null {
    return row.kind === 'group' ? row.group.protection : row.process.protection
  }

  function rowResident(row: TableRow): number | null {
    return row.kind === 'group' ? row.group.residentBytes : row.process.residentBytes
  }

  function rowFootprint(row: TableRow): number | null {
    return row.kind === 'group' ? row.group.footprintBytes : row.process.footprintBytes
  }

  function rowVirtual(row: TableRow): number | null {
    if (row.kind === 'process') return row.process.virtualBytes
    return virtualByGroup.get(row.group.groupId) ?? null
  }

  function rowPid(row: TableRow): number {
    return row.kind === 'group' ? row.group.mainPid : row.process.pid
  }

  function rowName(row: TableRow): string {
    return row.kind === 'group' ? row.group.name : row.process.name
  }
</script>

<div class={`memory-table flex flex-col gap-2 ${className}`}>
  <div class="flex flex-wrap items-center gap-2">
    <div class="relative min-w-52 flex-1">
      <Search class="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground" />
      <Input
        class="pl-7"
        placeholder="Search name, bundle id, pid"
        aria-label="Search processes"
        value={query.search}
        oninput={(event) => onQueryChange({ ...query, search: event.currentTarget.value })}
      />
    </div>

    <div class="flex items-center gap-1" role="group" aria-label="Filter processes">
      {#each FILTERS as filter (filter)}
        <Button
          variant={query.filter === filter ? 'secondary' : 'ghost'}
          size="xs"
          aria-pressed={query.filter === filter}
          onclick={() => onQueryChange({ ...query, filter })}
        >
          {FILTER_LABELS[filter]}
        </Button>
      {/each}
    </div>

    <div class="ml-auto flex items-center gap-1">
      {#if onIntervalChange}
        <div class="flex items-center gap-1" role="group" aria-label="Refresh interval">
          {#each INTERVALS as interval (interval)}
            <Button
              variant={intervalMs === interval ? 'secondary' : 'ghost'}
              size="xs"
              aria-pressed={intervalMs === interval}
              onclick={() => onIntervalChange?.(interval)}
            >
              {interval / 1000}s
            </Button>
          {/each}
        </div>
      {/if}
      {#if onTogglePause}
        <Button variant={paused ? 'secondary' : 'ghost'} size="xs" aria-pressed={paused} onclick={() => onTogglePause?.()}>
          {#if paused}<Play class="size-3.5" />{:else}<Pause class="size-3.5" />{/if}
          {paused ? 'Resume' : 'Pause'}
        </Button>
      {/if}
      <Button
        variant={showVirtual ? 'secondary' : 'ghost'}
        size="xs"
        aria-pressed={showVirtual}
        onclick={() => (showVirtual = !showVirtual)}
      >
        Virtual
      </Button>
    </div>
  </div>

  {#if cpuWarmingUp}
    <p class="text-xs text-muted-foreground">Measuring CPU…</p>
  {/if}

  <div
    bind:this={viewportEl}
    bind:clientHeight={viewportHeight}
    class="relative h-[30rem] overflow-auto rounded-md border"
    role="grid"
    aria-label="Processes"
    aria-rowcount={rows.length + 1}
    aria-colcount={columnCount}
    tabindex="0"
    onscroll={(event) => (scrollTop = event.currentTarget.scrollTop)}
    onkeydown={onKeydown}
    onclick={handleGridClick}
    ondblclick={handleGridDblClick}
    onpointerenter={() => (frozen = true)}
    onpointerleave={() => (frozen = false)}
    onfocusin={() => (frozen = true)}
    onfocusout={handleFocusOut}
  >
    <div role="rowgroup" class="sticky top-0 z-10 border-b bg-background">
      <div role="row" class="flex h-7 items-center text-xs text-muted-foreground">
        <div role="columnheader" class="w-8 shrink-0">
          <span class="sr-only">Select</span>
        </div>
        <div role="columnheader" aria-sort={sortAria('name')} class="flex min-w-0 flex-1 items-center px-2">
          <button type="button" class="flex items-center gap-1 hover:text-foreground" onclick={() => sortBy('name')}>
            Application
            {#if query.sort === 'name'}{#if query.desc}<ArrowDown class="size-3" />{:else}<ArrowUp class="size-3" />{/if}{/if}
          </button>
        </div>
        <div role="columnheader" aria-sort={sortAria('cpu')} class="flex w-14 items-center justify-end px-2">
          <button type="button" class="flex items-center gap-1 hover:text-foreground" onclick={() => sortBy('cpu')}>
            CPU
            {#if query.sort === 'cpu'}{#if query.desc}<ArrowDown class="size-3" />{:else}<ArrowUp class="size-3" />{/if}{/if}
          </button>
        </div>
        <div role="columnheader" aria-sort={sortAria('footprint')} class="flex w-24 items-center justify-end px-2">
          <button type="button" class="flex items-center gap-1 hover:text-foreground" onclick={() => sortBy('footprint')}>
            Memory
            {#if query.sort === 'footprint'}{#if query.desc}<ArrowDown class="size-3" />{:else}<ArrowUp class="size-3" />{/if}{/if}
          </button>
        </div>
        <div role="columnheader" aria-sort={sortAria('resident')} class="flex w-24 items-center justify-end px-2">
          <button type="button" class="flex items-center gap-1 hover:text-foreground" onclick={() => sortBy('resident')}>
            Real
            {#if query.sort === 'resident'}{#if query.desc}<ArrowDown class="size-3" />{:else}<ArrowUp class="size-3" />{/if}{/if}
          </button>
        </div>
        {#if showVirtual}
          <div role="columnheader" aria-sort={sortAria('virtual')} class="flex w-24 items-center justify-end px-2">
            <button type="button" class="flex items-center gap-1 hover:text-foreground" onclick={() => sortBy('virtual')}>
              Virtual
              {#if query.sort === 'virtual'}{#if query.desc}<ArrowDown class="size-3" />{:else}<ArrowUp class="size-3" />{/if}{/if}
            </button>
          </div>
        {/if}
        <div role="columnheader" aria-sort={sortAria('pid')} class="flex w-16 items-center justify-end px-2">
          <button type="button" class="flex items-center gap-1 hover:text-foreground" onclick={() => sortBy('pid')}>
            PID
            {#if query.sort === 'pid'}{#if query.desc}<ArrowDown class="size-3" />{:else}<ArrowUp class="size-3" />{/if}{/if}
          </button>
        </div>
        <div role="columnheader" class="w-20 px-2">Kind</div>
        <div role="columnheader" class="w-8">
          <span class="sr-only">Protection</span>
        </div>
      </div>
    </div>

    <div role="rowgroup" class="relative" style={`height: ${rows.length * ROW_HEIGHT}px`}>
      {#each visible as row, index (row.key)}
        {@render dataRow(row, startIndex + index)}
      {/each}
    </div>
  </div>

  <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
    {#if rows.length === 0}
      <span>No processes match this view.</span>
    {/if}
    {#if frozen}
      <span>Order paused while you point at the table or a row has focus.</span>
    {/if}
    {#if showRestrictedNote}
      <span
        >{restrictedCount} processes belong to other users or the system; their memory is not readable
        without admin rights.</span
      >
    {/if}
  </div>
</div>

{#snippet glyph(kind: ProcessKind)}
  {#if kind === 'app'}
    <AppWindow class="size-3.5 shrink-0 text-muted-foreground" />
  {:else if kind === 'helper'}
    <Boxes class="size-3.5 shrink-0 text-muted-foreground" />
  {:else if kind === 'agent'}
    <Bot class="size-3.5 shrink-0 text-muted-foreground" />
  {:else if kind === 'cli'}
    <Terminal class="size-3.5 shrink-0 text-muted-foreground" />
  {:else if kind === 'system'}
    <Server class="size-3.5 shrink-0 text-muted-foreground" />
  {:else}
    <Square class="size-3.5 shrink-0 text-muted-foreground" />
  {/if}
{/snippet}

{#snippet dataRow(row: TableRow, index: number)}
  {@const focused = index === focusIndex}
  {@const selected = selection.has(row.key)}
  {@const reason = protectionReason(rowProtection(row))}
  <div
    role="row"
    aria-rowindex={index + 2}
    aria-selected={selected}
    data-focused={focused}
    data-row-index={index}
    class={`absolute left-0 flex w-full items-center text-sm ${focused ? 'bg-muted/60' : ''}`}
    style={`top: ${index * ROW_HEIGHT}px; height: ${ROW_HEIGHT}px`}
  >
    <div role="gridcell" class="flex w-8 shrink-0 items-center justify-center">
      <Checkbox
        checked={selected}
        aria-label={`Select ${rowName(row)}`}
        onCheckedChange={() => onSelect(row.key)}
      />
    </div>

    <div
      role="gridcell"
      class={`flex min-w-0 flex-1 items-center gap-1.5 px-2 ${row.kind === 'process' ? 'pl-6' : ''}`}
    >
      {#if row.kind === 'group'}
        {#if row.group.pids.length > 1}
          <button
            type="button"
            class="shrink-0 text-muted-foreground hover:text-foreground"
            aria-expanded={expanded.has(row.group.groupId)}
            aria-label={`${expanded.has(row.group.groupId) ? 'Collapse' : 'Expand'} ${row.group.name}`}
            onclick={(event) => {
              event.stopPropagation()
              toggleExpand(row.group.groupId)
            }}
          >
            {#if expanded.has(row.group.groupId)}
              <ChevronDown class="size-3.5" />
            {:else}
              <ChevronRight class="size-3.5" />
            {/if}
          </button>
        {:else}
          <span class="size-3.5 shrink-0"></span>
        {/if}
      {/if}
      {@render glyph(rowKind(row))}
      <span class={`truncate ${row.kind === 'process' ? 'text-muted-foreground' : 'font-medium'}`}>
        {rowName(row)}
      </span>
      {#if row.kind === 'group' && row.group.pids.length > 1}
        <Badge variant="secondary" class="shrink-0">{row.group.pids.length} processes</Badge>
      {/if}
      {#if row.kind === 'group' && row.group.frontmost}
        <Badge variant="outline" class="shrink-0">Frontmost</Badge>
      {/if}
      {#if reason}
        <Lock class="size-3.5 shrink-0 text-muted-foreground" aria-label={reason} title={reason} />
      {/if}
    </div>

    <div role="gridcell" class="w-14 px-2 text-right tabular-nums">{cpuText(row.kind === 'group' ? row.group.cpuPercent : row.process.cpuPercent)}</div>
    <div role="gridcell" class="w-24 px-2 text-right tabular-nums">{memoryText(rowFootprint(row), rowResident(row))}</div>
    <div role="gridcell" class="w-24 px-2 text-right tabular-nums">{rowResident(row) === null ? '—' : formatBytes(rowResident(row) ?? 0)}</div>
    {#if showVirtual}
      <div role="gridcell" class="w-24 px-2 text-right tabular-nums">{rowVirtual(row) === null ? '—' : formatBytes(rowVirtual(row) ?? 0)}</div>
    {/if}
    <div role="gridcell" class="w-16 px-2 text-right tabular-nums">{rowPid(row)}</div>
    <div role="gridcell" class="w-20 px-2">{KIND_LABEL[rowKind(row)]}</div>
    <div role="gridcell" class="flex w-8 items-center justify-center">
      {#if reason}
        <Lock class="size-3.5 text-muted-foreground" aria-label={reason} title={reason} />
      {/if}
    </div>
  </div>
{/snippet}
