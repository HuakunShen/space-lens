<script lang="ts">
  import type { MemorySnapshot, ProcessRow } from '@space-lens/contract'
  import { formatBytes } from '@space-lens/web-ui/lib/memory/format'
  import { buildMemorySlice, processLabel, type Branch } from './chart-model.ts'

  let { snapshot, focusPid = null, selectedPid = null, onSelect, depth = $bindable(2), onFocus, onDetails }: { snapshot: MemorySnapshot; focusPid?: number | null; selectedPid?: number | null; onSelect: (pid: number) => void; depth?: number; onFocus: (pid: number | null) => void; onDetails: (row: ProcessRow) => void } = $props()
  let merge = $state(true)
  let hovered = $state<Branch | null>(null)
  let selectedGroup = $state<Branch | null>(null)
  const colors = ['#e452ba', '#aa65ef', '#8277f4', '#4d9bea', '#31bdc3', '#38b994', '#d3a052', '#df797b', '#9b87c7', '#6ba7b8', '#c8759d', '#94b670']
  let model = $derived(buildMemorySlice(snapshot, focusPid, depth, merge))
  let rows = $derived(new Map(snapshot.processes.map((p) => [p.pid, p])))
  let focusedRow = $derived(focusPid === null ? null : rows.get(focusPid) ?? null)
  let parent = $derived(focusedRow?.ppid != null ? rows.get(focusedRow.ppid) ?? null : null)
  let pinned = $state<Branch | null>(null)
  let lastSelected: number | null = null
  $effect(() => { if (selectedPid !== lastSelected) { lastSelected = selectedPid; if (!pinned?.pids.includes(selectedPid!)) pinned = null } })
  let selectedRow = $derived(selectedPid === null ? null : rows.get(selectedPid) ?? null)
  let inspected = $derived(hovered ?? pinned ?? (selectedRow ? { id: String(selectedRow.pid), name: processLabel(selectedRow, selectedRow.name), bytes: selectedRow.footprintBytes ?? selectedRow.residentBytes ?? 0, ownBytes: selectedRow.footprintBytes, pids: [selectedRow.pid], children: [], kind: 'process' as const } : null))
  let inspectedRow = $derived(inspected?.pids.length === 1 ? rows.get(inspected.pids[0]!) ?? null : null)
  type Segment = { branch: Branch; path: string; color: string; level: number }
  function point(r: number, a: number): [number, number] { return [150 + r * Math.cos(a - Math.PI / 2), 150 + r * Math.sin(a - Math.PI / 2)] }
  function wedge(start: number, end: number, inner: number, outer: number): string {
    end = Math.min(end, start + Math.PI * 2 - 0.00001)
    const [x1,y1] = point(outer,start), [x2,y2] = point(outer,end), [x3,y3] = point(inner,end), [x4,y4] = point(inner,start)
    return `M ${x1} ${y1} A ${outer} ${outer} 0 ${end - start > Math.PI ? 1 : 0} 1 ${x2} ${y2} L ${x3} ${y3} A ${inner} ${inner} 0 ${end - start > Math.PI ? 1 : 0} 0 ${x4} ${y4} Z`
  }
  function segments(branches: Branch[], rings: number): Segment[] {
    const output: Segment[] = []
    if (!model.total) return output
    const draw = (items: Branch[], start: number, span: number, level: number, parentColor?: string) => {
      const total = items.reduce((sum, item) => sum + item.bytes, 0)
      let angle = start
      for (const [index, item] of items.entries()) {
        const next = angle + span * item.bytes / total
        const color = parentColor ?? colors[index % colors.length]!
        if (next - angle > 0.001) output.push({ branch: item, color, level, path: wedge(angle, next, 43 + (level - 1) * 96 / rings, 43 + level * 96 / rings - 1) })
        if (level < rings && item.children.length) draw(item.children, angle, next - angle, level + 1, color)
        angle = next
      }
    }
    draw(branches, 0, Math.PI * 2, 1)
    return output
  }
  function actualDepth(items: Branch[]): number { return items.reduce((n, item) => Math.max(n, 1 + (item.children.length ? actualDepth(item.children) : 0)), 1) }
  let sunburst = $derived(segments(model.branches, Math.min(depth, actualDepth(model.branches))))
  let pie = $derived(segments(model.branches, 1))
  function select(item: Branch) {
    pinned = item
    if (item.pids.length > 0) onSelect(item.pids[0]!)
    if (item.pids.length > 1) selectedGroup = item
  }
  function explore(item: Branch) {
    if (item.kind === 'self' || item.pids.length === 0) return
    hovered = null
    if (item.pids.length === 1) { selectedGroup = null; onFocus(item.pids[0]!) }
    else selectedGroup = item
  }
  function executable(row: ProcessRow | null): string | null {
    return row?.command?.match(/^(.+?\/Contents\/MacOS\/\S+|\S+)/)?.[1] ?? null
  }
  function describe(row: ProcessRow | null): string {
    if (!row) return 'Select a segment to explore its processes.'
    return `PID ${row.pid} · ${row.kind} · ${row.state} · CPU ${row.cpuPercent === null ? '—' : `${row.cpuPercent.toFixed(1)}%`}`
  }
</script>
<section class="rounded-lg border bg-card p-4 shadow-sm" aria-label="Memory distribution">
  <div class="flex flex-wrap items-start gap-2">
    <div class="mr-auto min-w-0"><h2 class="text-sm font-semibold">Memory explorer</h2><p class="mt-1 text-xs text-muted-foreground">{focusedRow ? `${processLabel(focusedRow, focusedRow.name)} · PID ${focusedRow.pid}` : 'All processes'} · footprint, or resident if unavailable</p></div>
    {#if focusPid !== null}<button class="rounded-md border px-2 py-1 text-xs hover:bg-muted" onclick={() => { selectedGroup = null; onFocus(parent?.pid ?? null) }}>← {parent ? 'Parent' : 'All processes'}</button><button class="rounded-md border px-2 py-1 text-xs hover:bg-muted" onclick={() => { selectedGroup = null; onFocus(null) }}>All</button>{/if}
  </div>
  <div class="mt-3 flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
    <label class="flex items-center gap-1.5">Depth <select class="rounded border bg-background px-2 py-1 text-foreground" bind:value={depth}><option value={1}>1 level</option><option value={2}>2 levels</option><option value={3}>3 levels</option></select></label>
    <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={merge} /> Merge similar siblings</label>
    <span>Click to select · double-click to explore</span>
  </div>
  {#if focusedRow && model.branches.every((branch) => branch.kind === 'self')}<p class="mt-2 text-xs text-muted-foreground">Leaf process · no child processes in this snapshot.</p>{/if}
  {#if focusPid !== null && !focusedRow}<p class="mt-3 rounded border p-3 text-xs text-muted-foreground">This process is no longer in the snapshot. Choose All to continue.</p>{:else if model.total > 0}
    <div class="distribution-plots mt-3">
      {#each [{ name: 'Sunburst', parts: sunburst }, { name: 'Pie', parts: pie }] as chart}
        <div class="min-w-0 rounded-lg border bg-background/50 p-2">
          <div class="px-1 text-xs font-medium text-muted-foreground">{chart.name}</div>
          <svg viewBox="0 0 300 300" class="mx-auto block w-full" role="group" aria-label={`${chart.name} memory distribution`}>
            {#each chart.parts as segment}
              <path role="button" tabindex={segment.branch.kind === 'self' ? -1 : 0} aria-label={`${segment.branch.name}: ${formatBytes(segment.branch.bytes)}`} onfocus={() => hovered = segment.branch} onblur={() => hovered = null} onkeydown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); explore(segment.branch) } }} d={segment.path} fill={segment.color} opacity={hovered && hovered.id !== segment.branch.id ? 0.48 : (segment.level > 1 ? 0.78 : 1)} stroke={selectedPid !== null && segment.branch.pids.includes(selectedPid) ? 'var(--foreground)' : 'var(--background)'} stroke-width={selectedPid !== null && segment.branch.pids.includes(selectedPid) ? 2 : 1} class:cursor-pointer={segment.branch.kind !== 'self'} onmouseenter={() => hovered = segment.branch} onmouseleave={() => hovered = null} onclick={() => select(segment.branch)} ondblclick={() => explore(segment.branch)}><title>{segment.branch.name}: {formatBytes(segment.branch.bytes)}</title></path>
            {/each}
            <text x="150" y="147" text-anchor="middle" fill="currentColor" font-size="14" pointer-events="none">{formatBytes(model.total)}</text><text x="150" y="164" text-anchor="middle" fill="currentColor" opacity="0.6" font-size="9" pointer-events="none">footprint</text>
          </svg>
        </div>
      {/each}
    </div>
    <div class="mt-3 rounded-md border bg-muted/30 p-3 text-xs" aria-live="polite">
      {#if inspected}
        <div class="flex justify-between gap-2"><strong class="truncate text-sm">{inspected.name}</strong><strong class="shrink-0 tabular-nums">{formatBytes(inspected.bytes)}</strong></div>
        <p class="mt-1 text-muted-foreground">{(inspected.bytes / model.total * 100).toFixed(1)}% of current view · {inspected.pids.length > 1 ? `${inspected.pids.length} processes` : describe(inspectedRow)}</p>
        {#if inspectedRow}<p class="mt-1 text-muted-foreground">User {inspectedRow.user ?? 'unknown'} · Threads {inspectedRow.threads ?? '—'} · Parent PID {inspectedRow.ppid ?? '—'}</p>{#if executable(inspectedRow)}<p class="mt-1 break-all font-mono text-muted-foreground">{executable(inspectedRow)}</p>{/if}{/if}
        <p class="mt-1 text-muted-foreground">{inspected.kind === 'other' ? `${inspected.omitted} smaller items combined · click to browse` : inspected.kind === 'self' ? 'Memory of this process alone' : inspected.pids.length > 1 ? 'Click to choose an instance' : 'Selected process · double-click to explore'}</p>
      {:else if focusedRow}
        <div class="flex justify-between gap-2"><strong>{processLabel(focusedRow, focusedRow.name)}</strong><strong>{formatBytes(model.total)}</strong></div><p class="mt-1 text-muted-foreground">{describe(focusedRow)} · self {focusedRow.footprintBytes === null ? 'unavailable' : formatBytes(focusedRow.footprintBytes)}</p>
        <p class="mt-1 text-muted-foreground">User {focusedRow.user ?? 'unknown'} · Threads {focusedRow.threads ?? '—'} · Parent PID {focusedRow.ppid ?? '—'}</p>
        {#if executable(focusedRow)}<p class="mt-1 break-all font-mono text-muted-foreground">{executable(focusedRow)}</p>{/if}
        <button class="mt-2 rounded border px-2 py-1 hover:bg-muted" onclick={() => onDetails(focusedRow!)}>Process details</button>
      {:else}<strong>Explore memory</strong><p class="mt-1 text-muted-foreground">Hover a segment to inspect it. Click to keep a selection. Double-click to explore child processes.</p>{/if}
    </div>
    {#if selectedGroup}
      <div class="mt-3 rounded-md border p-2"><div class="flex items-center justify-between px-1 text-xs font-medium"><span>{selectedGroup.name} · choose a process</span><button onclick={() => selectedGroup = null} aria-label="Close process choices">✕</button></div>
        {#if selectedGroup.pids.length > 200}<p class="px-1 text-xs text-muted-foreground">Showing 200 of {selectedGroup.pids.length} processes.</p>{/if}<div class="mt-1 max-h-40 overflow-auto">{#each selectedGroup.pids.slice(0, 200) as pid}{#if rows.get(pid)}{@const row = rows.get(pid)!}<button class="flex w-full items-center justify-between rounded px-2 py-1.5 text-left text-xs hover:bg-muted" onclick={() => { selectedGroup = null; onSelect(pid) }} ondblclick={() => onFocus(pid)}><span>PID {pid} · {row.name}</span><span>{row.footprintBytes === null ? '—' : formatBytes(row.footprintBytes)}</span></button>{/if}{/each}</div>
      </div>
    {/if}
    <div class="mt-3 max-h-64 overflow-auto" aria-label="Memory distribution items">
      {#each model.branches as item, i}
        <button class="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs hover:bg-muted disabled:cursor-default" disabled={item.kind === 'self' || item.pids.length === 0} onmouseenter={() => hovered = item} onmouseleave={() => hovered = null} onfocus={() => hovered = item} onblur={() => hovered = null} onclick={() => select(item)} ondblclick={() => explore(item)}><span class="h-2.5 w-2.5 shrink-0 rounded-sm" style:background={colors[i % colors.length]}></span><span class="min-w-0 flex-1 truncate">{item.name}</span><span class="text-muted-foreground">{item.pids.length > 1 ? `${item.pids.length} processes` : ''}</span><span class="w-20 text-right tabular-nums">{formatBytes(item.bytes)}</span><span class="text-muted-foreground">›</span></button>
      {/each}
    </div>
  {:else}<p class="py-10 text-center text-sm text-muted-foreground">No footprint data available.</p>{/if}
</section>

<style>
  section { container-type: inline-size; }
  .distribution-plots { display: grid; gap: .75rem; grid-template-columns: repeat(2, minmax(0, 1fr)); }
  svg { max-height: 300px; }
  path:focus, path:focus-visible { outline: none !important; }
  path { transition: opacity 100ms ease; }
  @container (max-width: 320px) { .distribution-plots { grid-template-columns: minmax(0, 1fr); } }
</style>
