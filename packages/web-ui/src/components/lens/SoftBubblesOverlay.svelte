<!-- Temporary Canvas ownership during a bubble drag; ECharts owns every resting chart. -->
<script lang="ts">
  import { untrack } from 'svelte'
  import type { BubbleCircle } from '../../lib/bubbles'
  import type { ChartSize } from '../../lib/chart-size'
  import { buildBubbleLabels } from '../../lib/bubble-label'
  import { SoftBubbleWorld } from '../../lib/soft-bubbles/world'
  import { paintSoftBubbles } from '../../lib/soft-bubbles/paint'
  import type { Point } from '../../lib/soft-bubbles/geometry'

  interface Props {
    circles: readonly BubbleCircle[]
    offset: number
    size: ChartSize
    enabled: boolean
    collectedIds: Set<string>
    onActiveChange: (active: boolean) => void
    onDragChange: (id: string | null) => void
    onActivated: () => void
    onReleased: () => void
  }
  let { circles, offset, size, enabled, collectedIds, onActiveChange, onDragChange, onActivated, onReleased }: Props = $props()
  let canvas = $state<HTMLCanvasElement>()
  let active = $state(false)
  let world: SoftBubbleWorld | null = null
  let frame = 0
  let lastTime = 0
  let accumulator = 0
  let restFrames = 0
  let measuredFrames = 0
  let measuredSteps = 0
  let measuredTime = 0
  let maxSteps = 3
  let draggingId: string | null = null
  let pending: { id: string; pointerId: number; start: Point; shift: Point; bounds: DOMRect } | null = null
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
    const samples = circles.some((circle) => circle.r > 400) ? 128 : 64
    paintSoftBubbles(context, world.snapshot(samples), { ...size, offset, labels, collectedIds, hoveredId: draggingId })
  }

  function finish(): void {
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
    measuredFrames = 0
    measuredSteps = 0
    measuredTime = 0
    maxSteps = 3
  }

  function tick(time: number): void {
    frame = 0
    if (!world || document.hidden) { cancel(); return }
    const elapsed = lastTime ? Math.min(0.05, (time - lastTime) / 1000) : 0
    lastTime = time
    accumulator += elapsed
    let steps = 0
    const start = performance.now()
    while (accumulator >= STEP && steps < maxSteps) {
      world.step(STEP)
      accumulator -= STEP
      steps++
    }
    accumulator = Math.min(accumulator, STEP * maxSteps)
    if (measuredFrames < 30) {
      measuredTime += performance.now() - start
      measuredSteps += steps
      measuredFrames++
      if (measuredFrames === 30 && measuredSteps > 0 && measuredTime / measuredSteps > 4) maxSteps = 1
    }
    draw()
    restFrames = pending === null && world.isAtRest() ? restFrames + 1 : 0
    if (restFrames >= 10) { finish(); return }
    frame = requestAnimationFrame(tick)
  }

  function removeListeners(): void {
    window.removeEventListener('pointermove', move)
    window.removeEventListener('pointerup', release)
    window.removeEventListener('pointercancel', cancel)
    window.removeEventListener('blur', cancel)
  }

  export function cancel(): void {
    const hadGesture = pending !== null || active
    pending = null
    removeListeners()
    if (active || world) finish()
    if (hadGesture) onReleased()
  }

  export function arm(id: string, event: PointerEvent): void {
    if (!enabled || active || event.button !== 0 || !event.isPrimary || !canvas) return
    const circle = circles.find((entry) => entry.id === id)
    if (!circle || circle.isAggregate || circle.node.scanState === 'skipped') return
    cancel()
    const bounds = canvas.getBoundingClientRect()
    const start = { x: event.clientX - bounds.left, y: event.clientY - bounds.top }
    pending = { id, pointerId: event.pointerId, start, shift: { x: start.x - circle.x - offset, y: start.y - circle.y - offset }, bounds }
    window.addEventListener('pointermove', move, { passive: false })
    window.addEventListener('pointerup', release)
    window.addEventListener('pointercancel', cancel)
    window.addEventListener('blur', cancel)
  }

  function move(event: PointerEvent): void {
    if (!pending || event.pointerId !== pending.pointerId) return
    const point = { x: event.clientX - pending.bounds.left, y: event.clientY - pending.bounds.top }
    if (!active && Math.hypot(point.x - pending.start.x, point.y - pending.start.y) <= 3) return
    if (!active) {
      world = new SoftBubbleWorld(circles, offset)
      if (!world.drag(pending.id, { x: pending.start.x - pending.shift.x, y: pending.start.y - pending.shift.y })) { cancel(); return }
      draggingId = pending.id
      draw()
      active = true
      onActivated()
      onDragChange(draggingId)
      onActiveChange(true)
      frame = requestAnimationFrame(tick)
    }
    event.preventDefault()
    world?.drag(pending.id, { x: point.x - pending.shift.x, y: point.y - pending.shift.y })
  }

  function release(event: PointerEvent): void {
    if (!pending || event.pointerId !== pending.pointerId) return
    pending = null
    removeListeners()
    if (active) { world?.release(); onReleased() }
  }

  $effect(() => {
    void circles
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
