/** Paint the temporary soft-body layer using the static ECharts materials and seed labels. */
import { BUBBLE_FONT, type BubbleLabel } from '../bubble-label.ts'
import { BUBBLE_TEXT_COLOR, BUBBLE_TEXT_SHADOW, bubbleStyle, gradientInBounds } from '../chart-material.ts'
import type { Point } from './geometry.ts'
import type { SoftBubbleSnapshot } from './world.ts'

interface PaintOptions {
  theme?: 'dark' | 'light'
  width: number
  height: number
  offset: number
  labels: ReadonlyMap<string, BubbleLabel>
  collectedIds: ReadonlySet<string>
  hoveredId: string | null
}

function outline(context: CanvasRenderingContext2D, points: readonly Point[]): void {
  const first = points[0]
  const last = points.at(-1)
  if (!first || !last) return
  context.beginPath()
  context.moveTo((last.x + first.x) / 2, (last.y + first.y) / 2)
  for (let index = 0; index < points.length; index++) {
    const point = points[index]
    const next = points[(index + 1) % points.length]
    if (point && next) context.quadraticCurveTo(point.x, point.y, (point.x + next.x) / 2, (point.y + next.y) / 2)
  }
  context.closePath()
}

export function paintSoftBubbles(
  context: CanvasRenderingContext2D,
  bodies: readonly SoftBubbleSnapshot[],
  options: PaintOptions,
): void {
  context.save()
  context.shadowBlur = 0
  context.shadowOffsetY = 0
  context.clearRect(0, 0, options.width, options.height)
  // ECharts uses z2=0 for parents and z2=2 for children; their labels sit above all paths.
  for (const childLayer of [false, true]) {
    for (const body of bodies) {
      if ((body.parentId !== null) !== childLayer) continue
      const label = options.labels.get(body.id)
      if (!label || body.points.length === 0) continue
      const style = bubbleStyle(
        body.circle.color,
        label.hasVisibleChildren,
        options.collectedIds.has(body.id),
        options.hoveredId === body.id,
        options.theme,
      )
      let left = Infinity
      let top = Infinity
      let right = -Infinity
      let bottom = -Infinity
      for (const point of body.points) {
        left = Math.min(left, point.x)
        top = Math.min(top, point.y)
        right = Math.max(right, point.x)
        bottom = Math.max(bottom, point.y)
      }
      const gradientBox = gradientInBounds(style.fill, { x: left, y: top, width: right - left, height: bottom - top })
      const gradient = context.createLinearGradient(
        gradientBox.start.x,
        gradientBox.start.y,
        gradientBox.end.x,
        gradientBox.end.y,
      )
      for (const stop of gradientBox.colorStops) gradient.addColorStop(stop.offset, stop.color)
      outline(context, body.points)
      context.fillStyle = gradient
      context.fill()
    }
  }
  context.textAlign = 'center'
  context.textBaseline = 'middle'
  context.fillStyle = BUBBLE_TEXT_COLOR
  context.shadowColor = BUBBLE_TEXT_SHADOW
  context.shadowBlur = 3
  context.shadowOffsetY = 1
  for (const body of bodies) {
    const label = options.labels.get(body.id)
    if (!label?.text) continue
    const x = label.x + options.offset + body.cx - (body.circle.x + options.offset)
    const y = label.y + options.offset + body.cy - (body.circle.y + options.offset)
    const lines = label.text.split('\n')
    context.font = `${label.fontWeight} ${label.fontSize}px ${BUBBLE_FONT}`
    for (let index = 0; index < lines.length; index++) {
      const line = lines[index]
      if (line) context.fillText(line, x, y + (index - (lines.length - 1) / 2) * label.lineHeight)
    }
  }
  context.restore()
}
