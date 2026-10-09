<script lang="ts">
  import { fade } from 'svelte/transition'
  import { prefersReducedMotion } from 'svelte/motion'
  import type { TreeNodeSummary, TreeSliceNode } from '../../types'
  import { buildSunburstSegments } from '../../lib/sunburst'
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
    /** Carried so the toggle can flip the treemap's density from any chart. */
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
  let size = $state<ChartSize>({ width: 620, height: 430 })
  // The ring canvas follows the stage, so a wider workbench buys real ring
  // thickness and label room rather than just scaling the same drawing.
  let canvas = $derived({ width: Math.max(240, size.width), height: Math.max(240, size.height) })
  let radius = $derived(Math.max(80, Math.min(canvas.width, canvas.height) / 2 - 10))
  // The hole stays a fraction of the chart, so a small panel is not all well.
  let innerRadius = $derived(Math.max(38, Math.round(radius * 0.24)))
  let segments = $derived(tree ? buildSunburstSegments(tree, radius, innerRadius) : [])
  let active = $derived(segments.find((segment) => segment.id === hoveredId))
  let inspected = $derived(active?.node ?? hoveredNode ?? focusNode)
  let percentage = $derived(focusNode && focusNode.size > 0 && inspected ? (inspected.size / focusNode.size) * 100 : 0)
  let percentageLabel = $derived(
    percentage > 0 && percentage < 0.1 ? '<0.1%' : `${percentage.toFixed(percentage < 10 ? 1 : 0)}%`,
  )
</script>

<section class="chart-wrap" aria-label="Disk usage chart">
  <div class="chart-toolbar">
    <span class="chart-eyebrow">SPACE DISTRIBUTION</span>
    <ChartModeToggle {mode} onModeChange={onModeChange} {density} onDensityChange={onDensityChange} />
  </div>
  <ChartStage
    label="Sunburst disk usage chart"
    empty={segments.length === 0 ? chartEmptyMessage(focusNode) : null}
    {canGoBack}
    {onBack}
    bind:size
  >
    {#key `${tree?.id ?? ''}:${canvas.width}x${canvas.height}`}
      <svg
        class="sunburst" width="100%" height="100%"
        viewBox={`0 0 ${canvas.width} ${canvas.height}`}
        aria-label="Sunburst disk usage chart"
        in:fade={{ duration: prefersReducedMotion.current ? 0 : 220 }}
      >
        <g transform={`translate(${canvas.width / 2}, ${canvas.height / 2})`}>
          <circle class="center-well" r={innerRadius} />
          {#each segments as segment (segment.id)}
            <path
              class="arc"
              class:hovered={hoveredId === segment.id}
              class:dimmed={hoveredId !== null &&
                active !== undefined &&
                hoveredId !== segment.id &&
                !active.path.startsWith(`${segment.path}/`) &&
                !segment.path.startsWith(`${active.path}/`)}
              class:collected={collectedIds.has(segment.id)}
              in:fade={{
                duration: prefersReducedMotion.current ? 0 : 320,
                delay: prefersReducedMotion.current ? 0 : Math.min(segment.depth, 4) * 35,
              }}
              d={segment.pathData}
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
                if (
                  (event.key === 'Enter' || event.key === ' ') &&
                  !segment.isAggregate &&
                  segment.node.scanState !== 'skipped'
                ) {
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
                if (!segment.isAggregate && segment.node.scanState !== 'skipped')
                  onContext(segment.node, event.clientX, event.clientY)
              }}
            />
          {/each}
          <text class="center-size" text-anchor="middle" y="-6">{formatNodeSize(inspected)}</text>
          <text class="center-label" text-anchor="middle" y="18"
            >{inspected?.scanState === 'skipped'
              ? 'NOT SCANNED'
              : hoveredId && inspected
                ? percentageLabel + ' of folder'
                : 'MEASURED SIZE'}</text
          >
        </g>
      </svg>
    {/key}
  </ChartStage>
  <ChartInspector {inspected} {active} {hoveredId} {focusNode} {percentageLabel} noun="segment" />
</section>
