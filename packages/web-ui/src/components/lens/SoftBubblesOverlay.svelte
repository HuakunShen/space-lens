<!-- Canvas runs the POC membrane during interaction; ECharts receives the actual settled pose. -->
<script lang="ts">
  import { untrack } from 'svelte'
  import type { BubbleCircle } from '../../lib/bubbles'
  import type { ChartSize } from '../../lib/chart-size'
  import { buildBubbleLabels } from '../../lib/bubble-label'
  import { SoftBubbleWorld, bubbleAtPoint, type SoftBubbleSnapshot } from '../../lib/soft-bubbles/world'
  import { paintSoftBubbles } from '../../lib/soft-bubbles/paint'
  import type { Point } from '../../lib/soft-bubbles/geometry'

  interface Props {
    circles: readonly BubbleCircle[]
    offset: number
    rest: readonly SoftBubbleSnapshot[]
    theme: 'dark' | 'light'
    size: ChartSize
    enabled: boolean
    collectedIds: Set<string>
    onActiveChange: (active: boolean) => void
    onDragChange: (id: string | null) => void
    onActivated: () => void
    onRestChange: (rest: SoftBubbleSnapshot[]) => void
  }
  let { circles, offset, rest, theme, size, enabled, collectedIds, onActiveChange, onDragChange, onActivated, onRestChange }: Props = $props()
  let canvas = $state<HTMLCanvasElement>()
  let active = $state(false)
  let world: SoftBubbleWorld | null = null
  let frame = 0
  let lastTime = 0
  let accumulator = 0
  let restFrames = 0
  let draggingId: string | null = null
  let pending: { id: string; pointerId: number; start: Point; shift: Point; bounds: DOMRect; owner: HTMLElement; moved: boolean } | null = null
  const STEP = 1 / 120
  const labels = $derived(buildBubbleLabels(circles))

  function draw(): void {
    const context = canvas?.getContext('2d')
    if (!context || !world) return
    const ratio = Math.min(window.devicePixelRatio || 1, 2)
    if (canvas && (canvas.width !== Math.round(size.width * ratio) || canvas.height !== Math.round(size.height * ratio))) {
      canvas.width = Math.round(size.width * ratio)
      canvas.height = Math.round(size.height * ratio)
    }
    context.setTransform(ratio, 0, 0, ratio, 0, 0)
    const samples = 64
    paintSoftBubbles(context, world.snapshot(samples), { ...size, offset, labels, collectedIds, hoveredId: draggingId, theme })
  }

  function finish(publish = false): void {
    const settled = publish ? world?.snapshot() : undefined
    if (frame) cancelAnimationFrame(frame)
    frame = 0
    // Show the static chart before hiding the overlay in this same render batch.
    onActiveChange(false)
    active = false
    world = null
    draggingId = null
    onDragChange(null)
    lastTime = 0
    accumulator = 0
    restFrames = 0
    if (settled) onRestChange(settled)
  }

  function tick(time: number): void {
    frame = 0
    if (!world || document.hidden) { cancel(); return }
    const elapsed = lastTime ? Math.min(0.05, (time - lastTime) / 1000) : 0
    lastTime = time
    accumulator += elapsed
    let steps = 0
    const deadline = performance.now() + 8
    while (accumulator >= STEP && steps < 3) {
      world.step(STEP)
      accumulator -= STEP
      steps++
      if (performance.now() >= deadline) break
    }
    accumulator = Math.min(accumulator, STEP * 3)
    draw()
    restFrames = pending === null && world.isAtRest() ? restFrames + 1 : 0
    if (restFrames >= 2) { finish(true); return }
    frame = requestAnimationFrame(tick)
  }

  function removeListeners(): void {
    window.removeEventListener('pointermove', move)
    window.removeEventListener('pointerup', release)
    window.removeEventListener('pointercancel', cancel)
    window.removeEventListener('blur', cancel)
    if (pending?.owner.hasPointerCapture(pending.pointerId)) pending.owner.releasePointerCapture(pending.pointerId)
  }

  export function cancel(): void {
    removeListeners()
    pending = null
    if (active || world) finish()
  }

  function pointAt(event: PointerEvent, bounds: DOMRect): Point {
    return { x: (event.clientX - bounds.left) * size.width / bounds.width, y: (event.clientY - bounds.top) * size.height / bounds.height }
  }

  export function begin(event: PointerEvent): void {
    if (!enabled || event.button !== 0 || !event.isPrimary || !canvas) return
    const bounds = canvas.getBoundingClientRect()
    if (!bounds.width || !bounds.height) return
    const start = pointAt(event, bounds)
    const baseline = bubbleAtPoint(world?.snapshot() ?? rest, start, event.altKey)
    if (!baseline || baseline.circle.isAggregate || baseline.circle.node.scanState === 'skipped') return
    removeListeners()
    const owner = canvas.parentElement
    if (!owner) return
    pending = { id: baseline.id, pointerId: event.pointerId, start, shift: { x: start.x - baseline.cx, y: start.y - baseline.cy }, bounds, owner, moved: false }
    window.addEventListener('pointermove', move, { passive: false })
    window.addEventListener('pointerup', release)
    window.addEventListener('pointercancel', cancel)
    window.addEventListener('blur', cancel)
  }

  function move(event: PointerEvent): void {
    if (!pending || event.pointerId !== pending.pointerId) return
    const point = pointAt(event, pending.bounds)
    if (!pending.moved && Math.hypot(point.x - pending.start.x, point.y - pending.start.y) <= 3) return
    pending.moved = true
    // A plain click still reaches ECharts. Once it is a drag, capture keeps
    // text hits and moves outside the stage in this same gesture.
    if (!pending.owner.hasPointerCapture(event.pointerId)) pending.owner.setPointerCapture(event.pointerId)
    if (!active) {
      world = new SoftBubbleWorld(circles, offset, rest)
      if (!world.drag(pending.id, { x: point.x - pending.shift.x, y: point.y - pending.shift.y })) { cancel(); return }
      draggingId = pending.id
      world.step(STEP)
      draw()
      lastTime = performance.now()
      active = true
      onActivated()
      onDragChange(draggingId)
      onActiveChange(true)
      frame = requestAnimationFrame(tick)
    }
    event.preventDefault()
    draggingId = pending.id
    onActivated()
    onDragChange(draggingId)
    world?.drag(pending.id, { x: point.x - pending.shift.x, y: point.y - pending.shift.y })
  }

  function release(event: PointerEvent): void {
    if (!pending || event.pointerId !== pending.pointerId) return
    removeListeners()
    pending = null
    if (active) { world?.release(); draggingId = null }
  }

  $effect(() => {
    void circles
    void rest
    void size.width
    void size.height
    void enabled
    untrack(cancel)
  })
  $effect(() => {
    const hidden = () => { if (document.hidden) cancel() }
    document.addEventListener('visibilitychange', hidden)
    return () => { document.removeEventListener('visibilitychange', hidden); cancel() }
  })
</script>

<canvas bind:this={canvas} class:active class="soft-bubbles" aria-hidden="true"></canvas>

<style>
  .soft-bubbles { position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; opacity: 0; }
  .soft-bubbles.active { opacity: 1; }
</style>
