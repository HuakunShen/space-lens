<script lang="ts">
  import { fade } from 'svelte/transition'
  import { prefersReducedMotion } from 'svelte/motion'
  import type { TreeNodeSummary, TreeSliceNode } from '../../types'
  import { buildIcicleSegments, clipIcicleName, icicleColumns, type IcicleSegment } from '../../lib/icicle'
  import type { ChartMode, TreemapDensity } from '../../lib/chart-mode'
  import type { ChartSize } from '../../lib/chart-size'
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

  /** Band above the columns for the level headers. */
  const headerHeight = 24
  const levelTitles = ['Top-level folders', 'Subfolders', 'Deeper folders', 'Files & folders']

  let size = $state<ChartSize>({ width: 620, height: 430 })
  // Rows are as tall as the stage allows, which is what keeps a dense folder
  // from collapsing into unreadable slivers.
  let canvas = $derived({ width: Math.max(280, size.width), height: Math.max(160, size.height - headerHeight) })
  let segments = $derived(tree ? buildIcicleSegments(tree, canvas.width, canvas.height) : [])
  let columns = $derived(icicleColumns(segments, canvas.width))
  let active = $derived(segments.find((segment) => segment.id === hoveredId))
  let inspected = $derived(active?.node ?? hoveredNode ?? focusNode)
  let percentage = $derived(focusNode && focusNode.size > 0 && inspected ? (inspected.size / focusNode.size) * 100 : 0)
  let percentageLabel = $derived(
    percentage > 0 && percentage < 0.1 ? '<0.1%' : `${percentage.toFixed(percentage < 10 ? 1 : 0)}%`,
  )

  // Hovering keeps the hovered segment and its ancestor chain lit; unrelated
  // columns dim, so a path stands out from the rest of the partition.
  function isRelated(segment: IcicleSegment, hovered: IcicleSegment): boolean {
    return hovered.path.startsWith(`${segment.path}/`) || segment.path.startsWith(`${hovered.path}/`)
  }

  /**
   * Label lines with the baseline they sit on. Baselines are clamped inside
   * the segment: a thin row near the bottom of the chart used to have its name
   * painted past the canvas edge, which is what made the last rows look
   * chopped. A line that cannot fit at its natural offset moves up instead.
   */
  function labelLines(segment: IcicleSegment): Array<{ text: string; kind: 'name' | 'detail'; y: number }> {
    if (!segment.labelVisible) return []
    const top = segment.y0 + headerHeight
    const bottom = segment.y1 + headerHeight
    const span = segment.y1 - segment.y0
    const name = clipIcicleName(segment.name, segment.x1 - segment.x0)
    const share = (segment.share * 100).toFixed(segment.share * 100 < 10 ? 1 : 0)
    if (span >= 34) {
      return [
        { text: name, kind: 'name', y: Math.min(top + 15, bottom - 18) },
        { text: `${formatBytes(segment.size)} · ${share}%`, kind: 'detail', y: Math.min(top + 28, bottom - 4) },
      ]
    }
    return [{ text: name, kind: 'name', y: Math.min(top + 15, bottom - 4) }]
  }
</script>

<section class="chart-wrap" aria-label="Disk usage chart">
  <div class="chart-toolbar">
    <span class="chart-eyebrow">SPACE DISTRIBUTION</span>
    <ChartModeToggle {mode} onModeChange={onModeChange} {density} onDensityChange={onDensityChange} />
  </div>
  <ChartStage
    label="Icicle disk usage chart"
    empty={segments.length === 0 ? chartEmptyMessage(focusNode) : null}
    {canGoBack}
    {onBack}
    bind:size
  >
    {#key `${tree?.id ?? ''}:${canvas.width}x${canvas.height}`}
      <svg
        class="icicle"
        width="100%"
        height="100%"
        viewBox={`0 0 ${canvas.width} ${canvas.height + headerHeight}`}
        role="group"
        aria-label="Icicle disk usage chart"
        in:fade={{ duration: prefersReducedMotion.current ? 0 : 220 }}
      >
        {#each columns as column (column.depth)}
          <text class="icicle-level" x={column.x + 8} y="9">LEVEL {column.depth}</text>
          <text class="icicle-level-title" x={column.x + 8} y="20">{levelTitles[column.depth - 1] ?? ''}</text>
        {/each}
        {#each segments as segment (segment.id)}
          {@const lines = labelLines(segment)}
          <g class="tile-group" class:dimmed={hoveredId !== null && active !== undefined && !isRelated(segment, active)}>
            <rect
              class="arc"
              class:hovered={hoveredId === segment.id}
              class:collected={collectedIds.has(segment.id)}
              x={segment.x0}
              y={segment.y0 + headerHeight}
              width={Math.max(0, segment.x1 - segment.x0)}
              height={Math.max(0, segment.y1 - segment.y0)}
              rx="3"
              fill={segment.color}
              role="button"
              aria-disabled={segment.isAggregate || segment.node.scanState === 'skipped'}
              tabindex="0"
              aria-label={`${segment.name}, ${segment.isAggregate ? formatBytes(segment.size) : formatNodeSize(segment.node)}, ${segment.path}${segment.isAggregate ? ', summarized items' : ''}`}
              onmouseenter={() => onHover(segment.id)}
              onmouseleave={() => onHover(null)}
              onfocus={() => onHover(segment.id)}
              onblur={() => onHover(null)}
              onclick={() => {
                if (!segment.isAggregate && segment.node.scanState !== 'skipped') onOpen(segment.node)
              }}
              onkeydown={(event) => {
                if ((event.key === 'Enter' || event.key === ' ') && !segment.isAggregate && segment.node.scanState !== 'skipped') {
                  event.preventDefault()
                  onOpen(segment.node)
                }
                if (event.key === 'Escape') {
                  event.currentTarget.blur()
                  onHover(null)
                }
              }}
              oncontextmenu={(event) => {
                event.preventDefault()
                if (!segment.isAggregate && segment.node.scanState !== 'skipped') onContext(segment.node, event.clientX, event.clientY)
              }}
            />
            {#if lines.length > 0}
              <text
                class="tile-text tile-name"
                class:muted-ink={segment.node.ignored}
                x={segment.x0 + 8}
                y={lines[0].y}>{lines[0].text}</text
              >
              {#if lines.length > 1}
                <text
                  class="tile-text tile-detail"
                  class:muted-ink={segment.node.ignored}
                  x={segment.x0 + 8}
                  y={lines[1].y}>{lines[1].text}</text
                >
              {/if}
            {/if}
          </g>
        {/each}
      </svg>
    {/key}
  </ChartStage>
  <ChartInspector {inspected} {active} {hoveredId} {focusNode} {percentageLabel} noun="column" />
</section>
