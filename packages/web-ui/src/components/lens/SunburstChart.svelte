<script lang="ts">
  import { fade } from 'svelte/transition'
  import { prefersReducedMotion } from 'svelte/motion'
  import { Folder, File, ArrowUpLeft } from '@lucide/svelte'
  import type { TreeNodeSummary, TreeSliceNode } from '../../types'
  import { buildSunburstSegments } from '../../lib/sunburst'
  import { formatBytes } from '../../lib/format'
  import { formatNodeSize } from '../../lib/node-size'

  interface Props {
    tree: TreeSliceNode | null
    focusNode: TreeNodeSummary | null
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
    hoveredId,
    hoveredNode = null,
    collectedIds,
    onHover,
    onOpen,
    onContext,
    onBack,
    canGoBack = false,
  }: Props = $props()
  const size = 620
  let segments = $derived(tree ? buildSunburstSegments(tree, 285) : [])
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
  </div>
  <div class="chart-stage macos:md:flex-1 windows:md:flex-1 linux:md:flex-1">
    {#key tree?.id}
      <svg
        class="sunburst macos:md:absolute macos:md:inset-0 macos:md:h-full macos:md:max-w-none windows:md:absolute windows:md:inset-0 windows:md:h-full windows:md:max-w-none linux:md:absolute linux:md:inset-0 linux:md:h-full linux:md:max-w-none"
        viewBox={`0 0 ${size} ${size}`}
        role="group"
        aria-label="Sunburst disk usage chart"
        in:fade={{ duration: prefersReducedMotion.current ? 0 : 220 }}
      >
        <g transform={`translate(${size / 2}, ${size / 2})`}>
          <circle class="center-well" r="62" />
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
    {#if segments.length === 0}
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
      <p class="inspector-path">{inspected?.path ?? 'Hover or focus a segment to see its full path.'}</p>
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
