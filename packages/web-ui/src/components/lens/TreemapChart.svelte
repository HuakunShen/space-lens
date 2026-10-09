<script lang="ts">
  import { fade } from 'svelte/transition'
  import { prefersReducedMotion } from 'svelte/motion'
  import type { TreeNodeSummary, TreeSliceNode } from '../../types'
  import { buildTreemapTiles, clipTileName, type TreemapTile } from '../../lib/treemap'
  import type { ChartMode, TreemapDensity } from '../../lib/chart-mode'
  import type { ChartSize } from '../../lib/chart-size'
  import { chartEmptyMessage } from '../../lib/chart-empty'
  import { formatBytes } from '../../lib/format'
  import { formatNodeSize } from '../../lib/node-size'
  import ChartModeToggle from './ChartModeToggle.svelte'
  import ChartStage from './ChartStage.svelte'
  import ChartInspector from './ChartInspector.svelte'
  import ChartFolderGlyph from './ChartFolderGlyph.svelte'

  const paintId = $props.id()

  interface Props {
    tree: TreeSliceNode | null
    focusNode: TreeNodeSummary | null
    mode: ChartMode
    onModeChange: (mode: ChartMode) => void
    /** The treemap's second axis; the toggle lets the user flip it in place. */
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
  let nested = $derived(density === 'nested')
  // Tiles are laid out against the real stage, so a wide window gives each
  // tile the room its labels need instead of scaling a fixed 620x430 canvas.
  let canvas = $derived({ width: Math.max(240, size.width), height: Math.max(240, size.height) })
  let tiles = $derived(tree ? buildTreemapTiles(tree, canvas.width, canvas.height, nested) : [])
  let active = $derived(tiles.find((tile) => tile.id === hoveredId))
  let inspected = $derived(active?.node ?? hoveredNode ?? focusNode)
  let percentage = $derived(focusNode && focusNode.size > 0 && inspected ? (inspected.size / focusNode.size) * 100 : 0)
  let percentageLabel = $derived(
    percentage > 0 && percentage < 0.1 ? '<0.1%' : `${percentage.toFixed(percentage < 10 ? 1 : 0)}%`,
  )

  // Ancestor-path dimming mirrors the sunburst: hovering keeps the hovered
  // tile and its ancestor chain lit and dims unrelated tiles.
  function isRelated(tile: TreemapTile, hovered: TreemapTile): boolean {
    return (
      tile.id === hovered.id || hovered.path.startsWith(`${tile.path}/`) || tile.path.startsWith(`${hovered.path}/`)
    )
  }

  function tileLabelLines(tile: TreemapTile): Array<{ text: string; kind: 'name' | 'detail' }> {
    const width = tile.x1 - tile.x0
    const name = clipTileName(tile.name, width - (tile.label === 'strip' ? (width >= 180 ? 100 : 48) : 18))
    const detail = formatBytes(tile.size)
    if (tile.label === 'strip') {
      return [{ text: name, kind: 'name' }, { text: detail, kind: 'detail' }]
    }
    if (tile.label === 'card') {
      return [
        { text: name, kind: 'name' },
        { text: detail, kind: 'detail' },
      ]
    }
    if (tile.label === 'name') return [{ text: name, kind: 'name' }]
    return []
  }
</script>

<section class="chart-wrap" aria-label="Disk usage chart">
  <div class="chart-toolbar">
    <span class="chart-eyebrow">SPACE DISTRIBUTION</span>
    <ChartModeToggle {mode} onModeChange={onModeChange} {density} onDensityChange={onDensityChange} />
  </div>
  <ChartStage
    label={nested ? 'Nested treemap disk usage chart' : 'Flat treemap disk usage chart'}
    empty={tiles.length === 0 ? chartEmptyMessage(focusNode) : null}
    {canGoBack}
    {onBack}
    bind:size
  >
    {#key `${tree?.id ?? ''}:${nested}:${canvas.width}x${canvas.height}`}
      <svg
        class="treemap"
        width="100%"
        height="100%"
        viewBox={`0 0 ${canvas.width} ${canvas.height}`}
        aria-label={nested ? 'Nested treemap disk usage chart' : 'Flat treemap disk usage chart'}
        in:fade={{ duration: prefersReducedMotion.current ? 0 : 220 }}
      >
        <defs>
          {#each tiles as tile, index (tile.id)}
            <linearGradient id={`${paintId}-tile-${index}`} x1="0" y1="0" x2="0.9" y2="1" style={`--node: ${tile.color}`}>
              <stop class="tile-light" offset="0%" />
              <stop class="tile-shade" offset="100%" />
            </linearGradient>
            <clipPath id={`${paintId}-label-${index}`}>
              <rect x={tile.x0 + 3} y={tile.y0 + 3} width={Math.max(0, tile.x1 - tile.x0 - 6)} height={Math.max(0, tile.y1 - tile.y0 - 6)} />
            </clipPath>
          {/each}
        </defs>
        {#each tiles as tile, index (tile.id)}
          {@const lines = tileLabelLines(tile)}
          {@const center = { x: (tile.x0 + tile.x1) / 2, y: (tile.y0 + tile.y1) / 2 }}
          <g
            class="tile-group"
            class:dimmed={hoveredId !== null && active !== undefined && !isRelated(tile, active)}
          >
            <rect
              class="arc tile"
              class:hovered={hoveredId === tile.id}
              class:collected={collectedIds.has(tile.id)}
              x={tile.x0}
              y={tile.y0}
              width={tile.x1 - tile.x0}
              height={tile.y1 - tile.y0}
              rx={Math.min(9, (tile.x1 - tile.x0) / 5, (tile.y1 - tile.y0) / 5)}
              fill={`url(#${paintId}-tile-${index})`}
              role="button"
              aria-disabled={tile.isAggregate || tile.node.scanState === 'skipped'}
              tabindex="0"
              aria-label={`${tile.name}, ${tile.isAggregate ? formatBytes(tile.size) : formatNodeSize(tile.node)}, ${tile.path}${tile.isAggregate ? ', summarized items' : ''}`}
              onmouseenter={() => onHover(tile.id)}
              onmouseleave={() => onHover(null)}
              onfocus={() => onHover(tile.id)}
              onblur={() => onHover(null)}
              onclick={() => {
                if (!tile.isAggregate && tile.node.scanState !== 'skipped') onOpen(tile.node)
              }}
              onkeydown={(event) => {
                if ((event.key === 'Enter' || event.key === ' ') && !tile.isAggregate && tile.node.scanState !== 'skipped') {
                  event.preventDefault()
                  onOpen(tile.node)
                }
                if (event.key === 'Escape') {
                  event.currentTarget.blur()
                  onHover(null)
                }
              }}
              oncontextmenu={(event) => {
                event.preventDefault()
                if (!tile.isAggregate && tile.node.scanState !== 'skipped') onContext(tile.node, event.clientX, event.clientY)
              }}
            />
            {#if lines.length > 0}
              <g class="tile-labels" clip-path={`url(#${paintId}-label-${index})`}>
                {#if tile.label === 'strip'}
                  <ChartFolderGlyph x={tile.x0 + 9} y={tile.y0 + 9} size={25} />
                  <text class="tile-text tile-name" x={tile.x0 + 42} y={tile.y0 + 20}>{lines[0].text}</text>
                  <text class="tile-text tile-detail" x={tile.x0 + 42} y={tile.y0 + 35}>{lines[1]?.text}</text>
                  {#if tile.x1 - tile.x0 >= 180}
                    <rect class="tile-share-pill" x={tile.x1 - 48} y={tile.y0 + 12} width="38" height="21" rx="7" />
                    <text class="tile-text tile-share" text-anchor="middle" x={tile.x1 - 29} y={tile.y0 + 26}>{(tile.share * 100).toFixed(0)}%</text>
                  {/if}
                {:else}
                  {@const roomy = tile.label === 'card' && tile.y1 - tile.y0 >= 90 && tile.x1 - tile.x0 >= 100}
                  {@const left = tile.x0 + 12}
                  {@const baseline = roomy ? center.y + 12 : center.y - (lines.length > 1 ? 2 : -4)}
                  {#if roomy && tile.hasChildren && !tile.isAggregate}
                    <ChartFolderGlyph x={left} y={baseline - 48} size={30} />
                  {/if}
                  <text class="tile-text tile-name" class:muted-ink={tile.node.ignored} x={left} y={baseline}>{lines[0].text}</text>
                  {#if lines.length > 1}
                    <text class="tile-text tile-detail" class:muted-ink={tile.node.ignored} x={left} y={baseline + 17}>{lines[1].text}</text>
                  {/if}
                  {#if roomy && tile.x1 - tile.x0 >= 155}
                    <rect class="tile-share-pill" x={tile.x1 - 48} y={tile.y0 + 10} width="38" height="21" rx="7" />
                    <text class="tile-text tile-share" text-anchor="middle" x={tile.x1 - 29} y={tile.y0 + 24}>{(tile.share * 100).toFixed(0)}%</text>
                  {/if}
                {/if}
              </g>
            {/if}
          </g>
        {/each}
      </svg>
    {/key}
  </ChartStage>
  <ChartInspector {inspected} {active} {hoveredId} {focusNode} {percentageLabel} noun="tile" />
</section>
