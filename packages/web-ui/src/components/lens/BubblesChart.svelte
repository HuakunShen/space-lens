<script lang="ts">
  import { fade } from 'svelte/transition'
  import { prefersReducedMotion } from 'svelte/motion'
  import type { TreeNodeSummary, TreeSliceNode } from '../../types'
  import { buildBubbleCircles, clipBubbleName, type BubbleCircle } from '../../lib/bubbles'
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
  // A bigger stage packs the same circles larger, which is what makes the
  // names legible instead of clipped.
  let canvas = $derived({ width: Math.max(240, size.width), height: Math.max(240, size.height) })
  let circles = $derived(tree ? buildBubbleCircles(tree, canvas.width, canvas.height) : [])
  let active = $derived(circles.find((circle) => circle.id === hoveredId))
  let inspected = $derived(active?.node ?? hoveredNode ?? focusNode)
  let percentage = $derived(focusNode && focusNode.size > 0 && inspected ? (inspected.size / focusNode.size) * 100 : 0)
  let percentageLabel = $derived(
    percentage > 0 && percentage < 0.1 ? '<0.1%' : `${percentage.toFixed(percentage < 10 ? 1 : 0)}%`,
  )

  /** The slice root paints as a backdrop, never as a clickable circle. */
  let backdropId = $derived(tree?.id ?? null)

  /** Vertical room a label needs before it is worth painting at all. */
  const LABEL_BAND = 18

  /**
   * Where each circle's label can sit without landing on the circles packed
   * inside it.
   *
   * A packed parent's children sit in the middle of it, so the naive centre
   * or "top of the circle" anchor paints the name straight through a child.
   * Instead the clear band between the parent's edge and its topmost child
   * is measured — and if the pack left no such band, the name is dropped
   * rather than smeared over a child. The inspector still names the circle
   * on hover, so nothing becomes unidentifiable.
   */
  let labelSlots = $derived.by(() => {
    const slots = new Map<string, { anchor: number; room: number }>()
    for (const circle of circles) {
      let topmostChild = Number.POSITIVE_INFINITY
      let bottommostChild = Number.NEGATIVE_INFINITY
      let nested = false
      for (const other of circles) {
        if (other.id === circle.id) continue
        if (Math.hypot(other.x - circle.x, other.y - circle.y) + other.r <= circle.r + 0.5) {
          nested = true
          topmostChild = Math.min(topmostChild, other.y - other.r)
          bottommostChild = Math.max(bottommostChild, other.y + other.r)
        }
      }
      if (!nested) {
        slots.set(circle.id, { anchor: circle.y, room: circle.r * 2 })
        continue
      }
      const topBand = topmostChild - (circle.y - circle.r)
      const bottomBand = circle.y + circle.r - bottommostChild
      if (topBand >= LABEL_BAND) slots.set(circle.id, { anchor: circle.y - circle.r + topBand / 2, room: topBand })
      else if (bottomBand >= LABEL_BAND) slots.set(circle.id, { anchor: circle.y + circle.r - bottomBand / 2, room: bottomBand })
    }
    return slots
  })

  function isRelated(circle: BubbleCircle, hovered: BubbleCircle): boolean {
    return (
      circle.id === hovered.id ||
      hovered.path.startsWith(`${circle.path}/`) ||
      circle.path.startsWith(`${hovered.path}/`)
    )
  }

  function labelLines(circle: BubbleCircle): Array<{ text: string; kind: 'name' | 'detail' }> {
    const slot = labelSlots.get(circle.id)
    if (!circle.labelVisible || !slot) return []
    const anchor = labelAnchor(circle)
    // Clip to the chord at the name baseline. The full
    // diameter overestimates the space in a parent's header near the rim.
    const offset = Math.min(circle.r, Math.abs(anchor - circle.y) + 3)
    const chord = Math.sqrt(Math.max(0, circle.r ** 2 - offset ** 2))
    const name = clipBubbleName(circle.name, (chord * 12) / labelFontSize(circle))
    if (slot.room >= 38 && chord * 1.7 >= formatBytes(circle.size).length * 6) {
      return [
        { text: name, kind: 'name' },
        { text: formatBytes(circle.size), kind: 'detail' },
      ]
    }
    return [{ text: name, kind: 'name' }]
  }

  function labelFontSize(circle: BubbleCircle): number {
    return Math.min(17, Math.max(11, circle.r / 9))
  }

  /** The vertical centre of a circle's label block. */
  function labelAnchor(circle: BubbleCircle): number {
    const slot = labelSlots.get(circle.id)
    if (!slot) return circle.y
    return slot.anchor !== circle.y && slot.room >= 72 ? slot.anchor + 10 : slot.anchor
  }
</script>

<section class="chart-wrap" aria-label="Disk usage chart">
  <div class="chart-toolbar">
    <span class="chart-eyebrow">SPACE DISTRIBUTION</span>
    <ChartModeToggle {mode} onModeChange={onModeChange} {density} onDensityChange={onDensityChange} />
  </div>
  <ChartStage
    label="Packed bubbles disk usage chart"
    empty={circles.length === 0 ? chartEmptyMessage(focusNode) : null}
    {canGoBack}
    {onBack}
    bind:size
  >
    {#key `${tree?.id ?? ''}:${canvas.width}x${canvas.height}`}
      <svg
        class="bubbles"
        width="100%"
        height="100%"
        viewBox={`0 0 ${canvas.width} ${canvas.height}`}
        role="group"
        aria-label="Packed bubbles disk usage chart"
        in:fade={{ duration: prefersReducedMotion.current ? 0 : 220 }}
      >
        <defs>
          {#each circles as circle, index (circle.id)}
            <radialGradient id={`${paintId}-sphere-${index}`} cx="32%" cy="24%" r="78%" style={`--node: ${circle.color}`}>
              <stop class="sphere-light" offset="0%" />
              <stop class="sphere-color" offset="48%" />
              <stop class="sphere-shade" offset="100%" />
            </radialGradient>
          {/each}
        </defs>
        {#each circles as circle (circle.id)}
          {#if circle.id === backdropId}
            <circle class="bubble-backdrop" cx={circle.x} cy={circle.y} r={circle.r} />
          {/if}
        {/each}
        {#each circles as circle, index (circle.id)}
          {@const lines = labelLines(circle)}
          {#if circle.id !== backdropId}
            <g
              class="tile-group"
              class:dimmed={hoveredId !== null && active !== undefined && !isRelated(circle, active)}
            >
              <circle
                class="arc bubble"
                class:hovered={hoveredId === circle.id}
                class:collected={collectedIds.has(circle.id)}
                cx={circle.x}
                cy={circle.y}
                r={circle.r}
                fill={`url(#${paintId}-sphere-${index})`}
                role="button"
                aria-disabled={circle.isAggregate || circle.node.scanState === 'skipped'}
                tabindex="0"
                aria-label={`${circle.name}, ${circle.isAggregate ? formatBytes(circle.size) : formatNodeSize(circle.node)}, ${circle.path}${circle.isAggregate ? ', summarized items' : ''}`}
                onmouseenter={() => onHover(circle.id)}
                onmouseleave={() => onHover(null)}
                onfocus={() => onHover(circle.id)}
                onblur={() => onHover(null)}
                onclick={() => {
                  if (!circle.isAggregate && circle.node.scanState !== 'skipped') onOpen(circle.node)
                }}
                onkeydown={(event) => {
                  if ((event.key === 'Enter' || event.key === ' ') && !circle.isAggregate && circle.node.scanState !== 'skipped') {
                    event.preventDefault()
                    onOpen(circle.node)
                  }
                  if (event.key === 'Escape') {
                    event.currentTarget.blur()
                    onHover(null)
                  }
                }}
                oncontextmenu={(event) => {
                  event.preventDefault()
                  if (!circle.isAggregate && circle.node.scanState !== 'skipped') onContext(circle.node, event.clientX, event.clientY)
                }}
              />
              {#if lines.length > 0}
                {@const anchor = labelAnchor(circle)}
                {#if circle.hasChildren && !circle.isAggregate && circle.r >= 58 && (labelSlots.get(circle.id)?.room ?? 0) >= 72}
                  <ChartFolderGlyph x={circle.x - 15} y={anchor - 44} size={30} />
                {/if}
                <text
                  class="bubble-text bubble-name"
                  style={`font-size: ${labelFontSize(circle)}px`}
                  class:muted-ink={circle.node.ignored}
                  text-anchor="middle"
                  x={circle.x}
                  y={anchor + (lines.length > 1 ? -3 : 4)}>{lines[0].text}</text
                >
                {#if lines.length > 1}
                  <text
                    class="bubble-text bubble-detail"
                    style={`font-size: ${Math.max(10, labelFontSize(circle) - 2)}px`}
                    class:muted-ink={circle.node.ignored}
                    text-anchor="middle"
                    x={circle.x}
                    y={anchor + (labelFontSize(circle) >= 14 ? 18 : 14)}>{lines[1].text}</text
                  >
                {/if}
              {/if}
            </g>
          {/if}
        {/each}
      </svg>
    {/key}
  </ChartStage>
  <ChartInspector {inspected} {active} {hoveredId} {focusNode} {percentageLabel} noun="bubble" />
</section>
