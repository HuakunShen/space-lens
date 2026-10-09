/** Shared seed-space bubble labels keep the ECharts/Canvas handoff visually identical. */
import { clipBubbleName, type BubbleCircle } from './bubbles.ts'
import { formatNodeSize } from './node-size.ts'

export const BUBBLE_FONT = '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'

export interface BubbleLabel {
  text: string
  x: number
  y: number
  fontSize: number
  fontWeight: number
  lineHeight: number
  hasVisibleChildren: boolean
}

export function bubbleLabel(circle: BubbleCircle, children: readonly BubbleCircle[]): BubbleLabel {
  const hasVisibleChildren = children.length > 0
  const band = hasVisibleChildren
    ? Math.min(...children.map((child) => child.y - child.r)) - (circle.y - circle.r)
    : circle.r * 2
  const y = hasVisibleChildren ? circle.y - circle.r + band / 2 : circle.y
  const chord = Math.sqrt(Math.max(0, circle.r ** 2 - (Math.abs(y - circle.y) + 8) ** 2))
  const name = clipBubbleName(circle.name, chord)
  return {
    text:
      circle.labelVisible && circle.r >= 27 && band >= 22
        ? `${name}${circle.r >= 40 && band >= 40 ? `\n${formatNodeSize(circle.node)}` : ''}`
        : '',
    x: circle.x,
    y,
    fontSize: circle.r > 90 ? 14 : 12,
    fontWeight: hasVisibleChildren ? 600 : 500,
    lineHeight: 19,
    hasVisibleChildren,
  }
}

export function buildBubbleLabels(circles: readonly BubbleCircle[]): Map<string, BubbleLabel> {
  const byId = new Map(circles.map((circle) => [circle.id, circle]))
  const labels = new Map<string, BubbleLabel>()
  for (const circle of circles) {
    const children: BubbleCircle[] = []
    for (const child of circle.node.children) {
      const visible = byId.get(child.id)
      if (visible) children.push(visible)
    }
    labels.set(circle.id, bubbleLabel(circle, children))
  }
  return labels
}
