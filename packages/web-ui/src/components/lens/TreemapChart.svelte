<script lang="ts">
  import { fade } from 'svelte/transition'
  import { prefersReducedMotion } from 'svelte/motion'
  import { Folder, File, ArrowUpLeft } from '@lucide/svelte'
  import type { TreeNodeSummary, TreeSliceNode } from '../../types'
  import { buildTreemapTiles, clipTileName, type ChartMode, type TreemapTile } from '../../lib/treemap'
  import { formatBytes } from '../../lib/format'
  import { formatNodeSize } from '../../lib/node-size'
  import ChartModeToggle from './ChartModeToggle.svelte'

  interface Props {
    tree: TreeSliceNode | null
    focusNode: TreeNodeSummary | null
    /** The flat layer view when false, the nested inset view when true. */
    nested: boolean
    mode: ChartMode
    onModeChange: (mode: ChartMode) => void
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
    nested,
    mode,
    onModeChange,
    hoveredId,
    hoveredNode = null,
    collectedIds,
    onHover,
    onOpen,
    onContext,
    onBack,
    canGoBack = false,
  }: Props = $props()

  const width = 620
  const height = 430
  let tiles = $derived(tree ? buildTreemapTiles(tree, width, height, nested) : [])
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
      hovered.path.startsWith(`${tile.path}/`) || tile.path.startsWith(`${hovered.path}/`)
    )
  }

  function tileLabelLines(tile: TreemapTile): Array<{ text: string; kind: 'name' | 'detail' }> {
    const name = clipTileName(tile.name, tile.x1 - tile.x0)
    const detail = `${formatBytes(tile.size)} · ${(tile.share * 100).toFixed(tile.share * 100 < 10 ? 1 : 0)}%`
    if (tile.label === 'strip') {
      // The whole line clips — appending " · NN%" past the tile edge would
      // bleed into the neighbors.
      const line = clipTileName(
        `${tile.name} · ${(tile.share * 100).toFixed(0)}%`,
        tile.x1 - tile.x0,
      )
      return [{ text: line, kind: 'name' }]
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
    <ChartModeToggle {mode} onModeChange={onModeChange} />
  </div>
  <div class="chart-stage macos:md:flex-1 windows:md:flex-1 linux:md:flex-1">
    {#key `${tree?.id ?? ''}:${nested}`}
      <svg
        class="treemap macos:md:absolute macos:md:inset-0 macos:md:h-full macos:md:max-w-none windows:md:absolute windows:md:inset-0 windows:md:h-full windows:md:max-w-none linux:md:absolute linux:md:inset-0 linux:md:h-full linux:md:max-w-none"
        viewBox={`0 0 ${width} ${height}`}
        role="group"
        aria-label={nested ? 'Nested treemap disk usage chart' : 'Flat treemap disk usage chart'}
        in:fade={{ duration: prefersReducedMotion.current ? 0 : 220 }}
      >
        {#each tiles as tile (tile.id)}
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
              rx="3"
              fill={tile.color}
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
              {#if tile.label === 'strip'}
                <text
                  class="tile-text"
                  class:muted-ink={tile.node.ignored}
                  x={tile.x0 + 6}
                  y={tile.y0 + 11.5}>{lines[0].text}</text
                >
              {:else}
                <text
                  class="tile-text tile-name"
                  class:muted-ink={tile.node.ignored}
                  text-anchor="middle"
                  x={center.x}
                  y={center.y - (lines.length > 1 ? 2 : -4)}>{lines[0].text}</text
                >
                {#if lines.length > 1}
                  <text
                    class="tile-text tile-detail"
                    class:muted-ink={tile.node.ignored}
                    text-anchor="middle"
                    x={center.x}
                    y={center.y + 12}>{lines[1].text}</text
                  >
                {/if}
              {/if}
            {/if}
          </g>
        {/each}
      </svg>
    {/key}
    {#if tiles.length === 0}
      <div class="chart-empty">
        {focusNode?.scanState === 'skipped'
          ? 'This location was not scanned'
          : focusNode
            ? 'No child items to display'
            : 'Choose a folder to explore'}
      </div>
    {/if}
    {#if canGoBack}
      <button class="chart-back" type="button" onclick={onBack} aria-label="Go to parent folder"
        ><ArrowUpLeft size={14} /> Up one level</button
      >
    {/if}
  </div>
  <div
    class="chart-inspector macos:rounded-md macos:border-0 macos:border-t macos:bg-transparent macos:px-1 macos:pt-3 windows:rounded-lg windows:bg-card linux:rounded-xl linux:bg-card linux:p-4"
    class:inspecting={hoveredId !== null && inspected !== focusNode}
    aria-live="polite"
    aria-atomic="true"
  >
    <div
      class="inspector-icon macos:border-0 macos:bg-transparent macos:p-1 linux:rounded-lg"
      style={`--node: ${active?.color ?? 'var(--muted-foreground)'}`}
    >
      {#if inspected?.hasChildren || inspected?.collapsed}<Folder size={19} />{:else}<File size={19} />{/if}
    </div>
    <div class="inspector-content">
      <div class="inspector-heading">
        <strong>{inspected?.name ?? 'Explore your storage'}</strong><span>{formatNodeSize(inspected)}</span>
      </div>
      <p class="inspector-path">{inspected?.path ?? 'Hover or focus a tile to see its full path.'}</p>
      <p class="inspector-hint">
        {#if inspected?.scanState === 'skipped'}Not scanned · {inspected.skipReason ??
            'unavailable'}{:else if active?.isAggregate}{active.childCount.toLocaleString()} smaller items grouped here ·
          open the parent folder to explore{:else if hoveredId && inspected}{percentageLabel} of this folder · {inspected.hasChildren
            ? 'Click to explore'
            : 'Click to inspect'}{:else}Hover to inspect · click to explore{/if}
      </p>
    </div>
  </div>
</section>
