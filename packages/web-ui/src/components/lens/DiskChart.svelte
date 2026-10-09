<script lang="ts">
  import { untrack } from 'svelte'
  import { prefersReducedMotion } from 'svelte/motion'
  import { init, use, type EChartsType } from 'echarts/core'
  import { CustomChart, SunburstChart, TreemapChart } from 'echarts/charts'
  import { UniversalTransition, LabelLayout } from 'echarts/features'
  import { SVGRenderer } from 'echarts/renderers'
  import type { TreeNodeSummary, TreeSliceNode } from '../../types'
  import type { ChartMode, TreemapDensity } from '../../lib/chart-mode'
  import { buildDiskChartModel } from '../../lib/echarts-model'
  import { chartEmptyMessage } from '../../lib/chart-empty'
  import { formatNodeSize } from '../../lib/node-size'
  import SoftBubblesOverlay from './SoftBubblesOverlay.svelte'
  import ChartModeToggle from './ChartModeToggle.svelte'
  import ChartStage from './ChartStage.svelte'
  import ChartInspector from './ChartInspector.svelte'

  use([CustomChart, SunburstChart, TreemapChart, UniversalTransition, LabelLayout, SVGRenderer])

  interface Props {
    softBubbles?: boolean
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

  let { softBubbles = true, tree, focusNode, mode, onModeChange, density, onDensityChange, hoveredId,
    hoveredNode = null, collectedIds, onHover, onOpen, onContext, onBack, canGoBack = false }: Props = $props()
  let host = $state<HTMLDivElement>()
  let softOverlay = $state<{ arm(id: string, event: PointerEvent): void; cancel(): void }>()
  let softActive = $state(false)
  let pointerDown: PointerEvent | null = null
  let suppressClick = false
  let clickReset: ReturnType<typeof setTimeout> | undefined
  const softEnabled = $derived(softBubbles && mode === 'bubbles' && !prefersReducedMotion.current)

  function rememberPointer(event: PointerEvent): void {
    pointerDown = softEnabled && event.button === 0 && event.isPrimary ? event : null
  }
  function releaseGesture(): void {
    if (clickReset) clearTimeout(clickReset)
    clickReset = setTimeout(() => { suppressClick = false; clickReset = undefined }, 0)
    pointerDown = null
  }
  let chart = $state.raw<EChartsType>()
  let size = $state({ width: 620, height: 430 })
  const rendered: { width: number; height: number; tree: TreeSliceNode | null; mode: ChartMode | null; density: TreemapDensity | null } = {
    width: 0, height: 0, tree: null, mode: null, density: null,
  }
  const helpId = $props.id()
  let textColor = $state('#e8e8e8')
  $effect(() => {
    if (!host) return
    const updateColor = () => { if (host) textColor = getComputedStyle(host).color }
    updateColor()
    const observer = new MutationObserver(updateColor)
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['class', 'style', 'data-interface'] })
    return () => observer.disconnect()
  })
  let model = $derived(tree ? buildDiskChartModel({ tree, ...size, mode, density,
    reducedMotion: prefersReducedMotion.current, collectedIds, textColor }) : null)
  let active = $derived(hoveredId ? model?.nodes.get(hoveredId) : undefined)
  let inspected = $derived(active?.node ?? hoveredNode ?? focusNode)
  let percentage = $derived(focusNode?.size && inspected ? inspected.size / focusNode.size * 100 : 0)
  let percentageLabel = $derived(percentage > 0 && percentage < 0.1 ? '<0.1%' : `${percentage.toFixed(percentage < 10 ? 1 : 0)}%`)

  function eventId(data: unknown): string | null {
    return typeof data === 'object' && data !== null && 'id' in data && typeof data.id === 'string' ? data.id : null
  }
  function openNode(id: string | null): void {
    const item = id ? model?.nodes.get(id) : undefined
    if (item && !item.isAggregate && item.node.scanState !== 'skipped') onOpen(item.node)
  }
  function collectNode(id: string | null, x: number, y: number): void {
    const item = id ? model?.nodes.get(id) : undefined
    if (item && !item.isAggregate && item.node.scanState !== 'skipped') onContext(item.node, x, y)
  }

  // The instance survives mode and folder changes. Replacing the option lets
  // ECharts match stable node IDs across the old and new geometries.
  $effect(() => {
    if (!host) return
    const instance = init(host, undefined, { renderer: 'svg' })
    chart = instance
    instance.on('mouseover', (event) => { if (!softActive) onHover(eventId(event.data)) })
    instance.on('globalout', () => { if (!softActive) onHover(null) })
    instance.on('mousedown', (event) => {
      const id = eventId(event.data)
      if (softEnabled && id && pointerDown) softOverlay?.arm(id, pointerDown)
    })
    instance.on('click', (event) => {
      if (suppressClick || softActive) { suppressClick = false; return }
      openNode(eventId(event.data))
    })
    instance.on('contextmenu', (event) => {
      if (softActive) return
      const bounds = host?.getBoundingClientRect()
      collectNode(eventId(event.data), (bounds?.left ?? 0) + (event.event?.offsetX ?? 0), (bounds?.top ?? 0) + (event.event?.offsetY ?? 0))
    })
    return () => {
      untrack(() => softOverlay?.cancel())
      if (clickReset) clearTimeout(clickReset)
      instance.dispose()
      chart = undefined
    }
  })
  $effect(() => {
    if (!chart) return
    if (!model) { chart.clear(); return }
    const resized = rendered.width !== size.width || rendered.height !== size.height
    const changedGeometry = rendered.tree !== tree || rendered.mode !== mode || rendered.density !== density
    if (resized || changedGeometry) untrack(() => softOverlay?.cancel())
    if (resized) chart.resize({ width: size.width, height: size.height, silent: true })
    chart.setOption({ ...model.option, animation: changedGeometry && !prefersReducedMotion.current }, { notMerge: true })
    Object.assign(rendered, { ...size, tree, mode, density })
    // Selection and theme changes keep the keyboard target and inspector.
    untrack(() => { if (hoveredId && !model?.nodes.has(hoveredId)) onHover(null) })
  })
  $effect(() => {
    if (!chart || !model) return
    chart.dispatchAction({ type: 'downplay', seriesId: 'disk' })
    if (active) chart.dispatchAction({ type: 'highlight', seriesId: 'disk', dataIndex: active.dataIndex })
  })

  function keyboard(event: KeyboardEvent): void {
    const ids = model?.navigableIds ?? []
    if (!ids.length) return
    const index = hoveredId ? ids.indexOf(hoveredId) : -1
    if (['ArrowRight', 'ArrowDown', 'ArrowLeft', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault()
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? ids.length - 1
        : event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? (index - 1 + ids.length) % ids.length : (index + 1) % ids.length
      onHover(ids[next] ?? null)
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault(); openNode(hoveredId)
    } else if (event.key === 'Escape') {
      onHover(null)
    } else if (event.key === 'F10' && event.shiftKey) {
      event.preventDefault()
      const bounds = host?.getBoundingClientRect()
      collectNode(hoveredId, bounds?.left ?? 0, bounds?.top ?? 0)
    }
  }
</script>

<section class="chart-wrap" aria-label="Disk usage chart">
  <div class="chart-toolbar">
    <span class="chart-eyebrow">SPACE DISTRIBUTION</span>
    <ChartModeToggle {mode} {onModeChange} {density} {onDensityChange} />
  </div>
  <ChartStage class="disk-chart-stage" label={`${mode} disk usage chart`} empty={!model?.nodes.size ? chartEmptyMessage(focusNode) : null} {canGoBack} {onBack} bind:size>
    <!-- The visualization is a bounded keyboard application; its arrow/Enter controls are described below. -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div class="disk-chart" class:soft-enabled={softEnabled} style:opacity={softActive ? 0 : 1} onpointerdown={rememberPointer} bind:this={host} role="application" tabindex="0"
      aria-label={`${mode} disk usage visualization`} aria-describedby={helpId}
      onkeydown={keyboard} oncontextmenu={(event) => event.preventDefault()}
      onfocus={() => { if (!hoveredId) onHover(model?.navigableIds[0] ?? null) }} onblur={() => onHover(null)}></div>
    {#if mode === 'bubbles' && model?.bubbles}
      <SoftBubblesOverlay bind:this={softOverlay} circles={model.bubbles.circles} offset={model.bubbles.offset} {size}
        enabled={softEnabled} {collectedIds} onActiveChange={(value) => { softActive = value }}
        onDragChange={(id) => onHover(id)} onActivated={() => { suppressClick = true }} onReleased={releaseGesture} />
    {/if}
    {#if mode === 'sunburst' && model?.nodes.size}
      <div class="sunburst-center" aria-hidden="true" style={`--well-width: ${Math.max(0, (Math.min(size.width, size.height) / 2 - 10) * 0.48 - 12)}px`}>
        <strong>{formatNodeSize(inspected)}</strong>
        <span>{inspected?.scanState === 'skipped' ? 'NOT SCANNED' : hoveredId && inspected ? `${percentageLabel} of folder` : 'MEASURED SIZE'}</span>
      </div>
    {/if}
  </ChartStage>
  <p id={helpId} class="sr-only">Use arrow keys to inspect items, Enter to open, and Shift F10 to add or remove an item from review. The contents list provides the same files and folders.</p>
  <ChartInspector {inspected} {active} {hoveredId} {focusNode} {percentageLabel} noun="item" />
</section>

<style>
  .disk-chart { position: absolute; inset: 0; min-width: 0; overflow: hidden; border-radius: 8px; }
  .disk-chart.soft-enabled { touch-action: none; }
  .disk-chart:focus-visible { outline: 2px solid var(--muted-foreground); outline-offset: 3px; }
  .sunburst-center { position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%); width: var(--well-width); text-align: center; pointer-events: none; display: grid; gap: 6px; }
  .sunburst-center strong { color: var(--foreground); font-size: clamp(10px, calc(var(--well-width) / 5.5), 22px); font-weight: 600; line-height: 1.2; white-space: nowrap; }
  .sunburst-center span { color: var(--muted-foreground); font-size: clamp(6px, calc(var(--well-width) / 13), 10px); line-height: 1.3; letter-spacing: 0.04em; }
  @media (max-width: 760px) {
    /* The mobile layout scrolls instead of allocating a flex height. */
    :global(.chart-stage.disk-chart-stage) { min-height: min(75vw, 440px); flex: none; }
  }
</style>
