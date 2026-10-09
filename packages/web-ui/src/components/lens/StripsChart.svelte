<script lang="ts">
  import { fade } from 'svelte/transition'
  import { prefersReducedMotion } from 'svelte/motion'
  import { Folder, File, Lightbulb, ChevronRight } from '@lucide/svelte'
  import type { TreeNodeSummary, TreeSliceNode } from '../../types'
  import { buildStripRows, rankLargestItems, type StripRow, type StripSegment } from '../../lib/strips'
  import type { ChartMode, TreemapDensity } from '../../lib/chart-mode'
  import { chartEmptyMessage } from '../../lib/chart-empty'
  import { formatBytes } from '../../lib/format'
  import { formatNodeSize } from '../../lib/node-size'
  import ChartModeToggle from './ChartModeToggle.svelte'
  import ChartStage from './ChartStage.svelte'
  import ChartInspector from './ChartInspector.svelte'

  interface Props {
    tree: TreeSliceNode | null
    focusNode: TreeNodeSummary | null
    mode: ChartMode
    onModeChange: (mode: ChartMode) => void
    density: TreemapDensity
    onDensityChange: (density: TreemapDensity) => void
    hoveredId: string | null
    hoveredNode?: TreeNodeSummary | null
    collectedIds: Set<string>
    onHover: (id: string | null) => void
    onOpen: (node: TreeNodeSummary) => void
    onContext: (node: TreeNodeSummary, x: number, y: number) => void
    onBack?: () => void
    canGoBack?: boolean
  }

  let {
    tree,
    focusNode,
    mode,
    onModeChange,
    density,
    onDensityChange,
    hoveredId,
    hoveredNode = null,
    collectedIds,
    onHover,
    onOpen,
    onContext,
    onBack,
    canGoBack = false,
  }: Props = $props()

  /** How many entries the "largest items" rail ranks. */
  const RAIL_SIZE = 8
  /**
   * How many rows are painted before the tail is summarized. A real scan root
   * has dozens of entries, most of them kilobytes; painting every one of them
   * grows the panel to several screens and buries the rows that matter. The
   * tail is reported rather than dropped, in the same voice the treemap uses
   * for its grouped items.
   */
  const MAX_ROWS = 8

  let railExpanded = $state(false)
  let rows = $derived(tree ? buildStripRows(tree) : [])
  let ranked = $derived(tree ? rankLargestItems(tree, railExpanded ? RAIL_SIZE * 2 : RAIL_SIZE) : [])
  let visibleRows = $derived(rows.slice(0, MAX_ROWS))
  let hiddenRows = $derived(rows.slice(MAX_ROWS))
  let hiddenShare = $derived(hiddenRows.reduce((sum, row) => sum + row.share, 0))
  let active = $derived(
    rows.find((row) => row.id === hoveredId) ?? rows.flatMap((row) => row.segments).find((segment) => segment.id === hoveredId),
  )
  let inspected = $derived(active?.node ?? hoveredNode ?? focusNode)
  let percentage = $derived(focusNode && focusNode.size > 0 && inspected ? (inspected.size / focusNode.size) * 100 : 0)
  let percentageLabel = $derived(
    percentage > 0 && percentage < 0.1 ? '<0.1%' : `${percentage.toFixed(percentage < 10 ? 1 : 0)}%`,
  )

  /**
   * A real observation about this folder, derived from the rows rather than
   * written as copy: the top row and how much of the folder it accounts for.
   * Says nothing when there is only one row, because "X is the biggest of
   * one" is noise.
   */
  let insight = $derived.by(() => {
    const top = rows[0]
    if (!top || rows.length < 2) return null
    return `${top.name} is the largest folder here at ${formatBytes(top.size)}, ${percent(top.share)} of ${focusNode?.name ?? 'this folder'}.`
  })

  function percent(share: number): string {
    const value = share * 100
    if (value > 0 && value < 0.1) return '<0.1%'
    return `${value.toFixed(value < 10 ? 1 : 0)}%`
  }

  function segmentLabel(segment: StripSegment, row: StripRow): string {
    return `${segment.name}, ${formatNodeSize(segment.node)}, ${percent(segment.share)} of ${row.name}`
  }
</script>

<section class="chart-wrap" aria-label="Disk usage chart">
  <div class="chart-toolbar">
    <span class="chart-eyebrow">SPACE DISTRIBUTION</span>
    <ChartModeToggle {mode} onModeChange={onModeChange} {density} onDensityChange={onDensityChange} />
  </div>
  <ChartStage
    label="Proportional strips disk usage chart"
    empty={rows.length === 0 ? chartEmptyMessage(focusNode) : null}
    class="chart-stage-strips"
    {canGoBack}
    {onBack}
  >
    <div class="strips-layout">
        <div class="strips-main" in:fade={{ duration: prefersReducedMotion.current ? 0 : 220 }}>
          <ol class="strip-rows">
            {#each visibleRows as row, index (row.id)}
              <li class="strip-row" class:is-hovered={hoveredId === row.id} style={`--node: ${row.color}`}>
                <button
                  type="button"
                  class="strip-head"
                  aria-disabled={row.node.scanState === 'skipped'}
                  onclick={() => {
                    if (row.node.scanState !== 'skipped') onOpen(row.node)
                  }}
                  onmouseenter={() => onHover(row.id)}
                  onmouseleave={() => onHover(null)}
                  onfocus={() => onHover(row.id)}
                  onblur={() => onHover(null)}
                  oncontextmenu={(event) => {
                    event.preventDefault()
                    if (row.node.scanState !== 'skipped') onContext(row.node, event.clientX, event.clientY)
                  }}
                >
                  <span class="strip-chip" style={`--node: ${row.color}`}>
                    {#if row.hasChildren || row.collapsed}<Folder size={16} />{:else}<File size={16} />{/if}
                  </span>
                  <span class="strip-identity">
                    <span class="strip-name">{row.name}</span>
                    <span class="strip-meta">
                      <span class="strip-size">{row.node.scanState === 'skipped' ? formatNodeSize(row.node) : formatBytes(row.size)}</span>
                      <span class="strip-share">{row.size === 0 || row.node.scanState === 'skipped' ? '—' : percent(row.share)}</span>
                    </span>
                  </span>
                  <ChevronRight class="strip-chevron" size={14} aria-hidden="true" />
                </button>
                <div class="strip-bar" role="group" aria-label={`${row.name} breakdown`}>
                  {#if row.size === 0 || row.node.scanState === 'skipped'}
                    <!-- A location with nothing measured must not paint a
                         full-width bar: at a glance that reads as "biggest". -->
                    <span class="strip-piece strip-piece-empty"></span>
                  {:else if row.segments.length === 0}
                    <span class="strip-piece" style={`flex: 1; --node: ${row.color}`}></span>
                  {:else}
                    {#each row.segments as segment (segment.id)}
                      {@const labeled = segment.share >= 0.1 && !segment.node.ignored}
                      <button
                        type="button"
                        class="strip-piece"
                        class:hovered={hoveredId === segment.id}
                        class:collected={collectedIds.has(segment.id)}
                        class:muted-ink={segment.node.ignored}
                        style={`flex: ${Math.max(segment.share, 0.0001)}; --node: ${segment.color}`}
                        aria-disabled={segment.node.scanState === 'skipped'}
                        aria-label={segmentLabel(segment, row)}
                        title={segmentLabel(segment, row)}
                        onmouseenter={() => onHover(segment.id)}
                        onmouseleave={() => onHover(null)}
                        onfocus={() => onHover(segment.id)}
                        onblur={() => onHover(null)}
                        onclick={() => {
                          if (segment.node.scanState !== 'skipped') onOpen(segment.node)
                        }}
                        oncontextmenu={(event) => {
                          event.preventDefault()
                          if (segment.node.scanState !== 'skipped') onContext(segment.node, event.clientX, event.clientY)
                        }}
                      >
                        {#if labeled}
                          <span class="piece-label">{segment.name}</span>
                          <span class="piece-size">{formatBytes(segment.size)}</span>
                        {/if}
                      </button>
                    {/each}
                  {/if}
                </div>
              </li>
            {/each}
          </ol>
          {#if hiddenRows.length > 0}
            <p class="strip-more">
              {hiddenRows.length.toLocaleString()} smaller {hiddenRows.length === 1 ? 'folder' : 'folders'} ·
              {percent(hiddenShare)} of this folder · open a folder to explore
            </p>
          {/if}
        </div>
        <aside class="strips-rail" aria-label="Largest items">
          <div class="rail-head">
            <h3>Largest items</h3>
            <button
              type="button"
              class="rail-see-all"
              onclick={() => (railExpanded = !railExpanded)}>{railExpanded ? 'Less' : 'See All'}</button
            >
          </div>
          <ol>
            {#each ranked as item, index (item.id)}
              <li>
                <button
                  type="button"
                  class="rail-row"
                  class:is-hovered={hoveredId === item.id}
                  aria-disabled={item.node.scanState === 'skipped'}
                  onmouseenter={() => onHover(item.id)}
                  onmouseleave={() => onHover(null)}
                  onfocus={() => onHover(item.id)}
                  onblur={() => onHover(null)}
                  onclick={() => {
                    if (item.node.scanState !== 'skipped') onOpen(item.node)
                  }}
                >
                  <span class="rail-rank">{index + 1}</span>
                  <span class="rail-icon" style={`--node: ${item.color}`}>
                    {#if item.hasChildren}<Folder size={13} />{:else}<File size={13} />{/if}
                  </span>
                  <span class="rail-name">{item.name}</span>
                  <span class="rail-size">{formatBytes(item.size)}</span>
                </button>
              </li>
            {/each}
          </ol>
          {#if insight}
            <p class="rail-insight"><Lightbulb size={14} aria-hidden="true" /><span>{insight}</span></p>
          {/if}
        </aside>
    </div>
  </ChartStage>
  <ChartInspector {inspected} {active} {hoveredId} {focusNode} {percentageLabel} noun="row" />
</section>
