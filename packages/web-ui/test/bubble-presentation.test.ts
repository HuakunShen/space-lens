/** The Canvas handoff uses exactly the contours, labels, and gradients painted by ECharts. */
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { init, use } from 'echarts/core'
import { CustomChart } from 'echarts/charts'
import { SVGRenderer } from 'echarts/renderers'
import { buildDiskChartModel, BUBBLE_CANVAS_INSET } from '../src/lib/echarts-model.ts'
import { buildBubbleLabels } from '../src/lib/bubble-label.ts'
import { bubbleStyle, gradientInBounds } from '../src/lib/chart-material.ts'
import { bounds } from '../src/lib/soft-bubbles/geometry.ts'
import type { TreeSliceNode } from '../src/types.ts'

use([CustomChart, SVGRenderer])
const node = (id: string, size: number, children: TreeSliceNode[] = []): TreeSliceNode => ({
  id,
  name: id,
  path: `/root/${id}`,
  size,
  depth: 1,
  ignored: false,
  collapsed: false,
  hasChildren: children.length > 0,
  childCount: children.length,
  children,
  omittedBytes: 0,
  omittedCount: 0,
})

test('resting contours match actual ECharts paths and share label decisions', () => {
  const tree = node('root', 100, [node('folder', 80, [node('nested', 80)]), node('file', 20)])
  const model = buildDiskChartModel({
    tree,
    width: 720,
    height: 480,
    mode: 'bubbles',
    density: 'nested',
    reducedMotion: true,
  })
  assert.ok(model.bubbles)
  assert.equal(model.bubbles.offset, BUBBLE_CANVAS_INSET)
  const labels = buildBubbleLabels(model.bubbles.circles)
  assert.deepEqual(new Set(labels.keys()), new Set(model.nodes.keys()))
  assert.deepEqual([...labels.keys()].sort(), ['file', 'folder'], 'inner bubbles and their labels are omitted')
  assert.equal(labels.get('folder')?.hasVisibleChildren, false)
  assert.equal(labels.get('folder')?.y, model.bubbles.circles.find((circle) => circle.id === 'folder')?.y)
  const chart = init(null, undefined, { renderer: 'svg', ssr: true, width: 720, height: 480 })
  try {
    chart.setOption(model.option)
    const drawn = chart
      .getZr()
      .storage.getDisplayList(true)
      .filter((element) => element.type === 'path')
    assert.equal(drawn.length, model.bubbles.circles.length)
    for (const body of model.bubbles.rest) {
      const box = bounds(body.points)
      assert.ok(
        drawn.some((element) => {
          const actual = element.getBoundingRect()
          return (
            Math.abs(actual.x - box.x) < 1 &&
            Math.abs(actual.y - box.y) < 1 &&
            Math.abs(actual.width - box.width) < 2 &&
            Math.abs(actual.height - box.height) < 2
          )
        }),
        `${body.id} uses the settled contour, not a circular approximation`,
      )
    }
    const selected = buildDiskChartModel({
      tree,
      width: 720,
      height: 480,
      mode: 'bubbles',
      density: 'nested',
      reducedMotion: true,
      collectedIds: new Set(['folder']),
      theme: 'light',
    })
    assert.equal(selected.bubbles, model.bubbles, 'selection/theme reuse geometry instead of warming physics again')
  } finally {
    chart.dispose()
  }
})

test('relative gradients map both axes into the current deformed bounds', () => {
  const style = bubbleStyle('#365ba8', true, false)
  const gradient = gradientInBounds(style.fill, { x: 20, y: 40, width: 100, height: 160 })
  assert.deepEqual(gradient.start, { x: 20, y: 40 })
  assert.deepEqual(gradient.end, { x: 100, y: 200 })
  assert.deepEqual(gradient.colorStops, style.fill.colorStops)
  assert.equal(bubbleStyle('#365ba8', true, false, true).stroke, 'transparent')
  assert.equal(bubbleStyle('#365ba8', true, true).lineWidth, 0)
})

test('resting bubbles preserve sampled colors and remain borderless in both themes', () => {
  for (const theme of ['dark', 'light'] as const) {
    const style = bubbleStyle('#365ba8', true, false, false, theme)
    assert.equal(style.lineWidth, 0)
    assert.equal(style.stroke, 'transparent')
  }
  assert.deepEqual(
    bubbleStyle('#3f9af9', true, false, false, 'dark').fill,
    bubbleStyle('#3f9af9', true, false, false, 'light').fill,
  )
})

test('hover and dragging never introduce a new outline on a borderless bubble', () => {
  const resting = bubbleStyle('#365ba8', true, false)
  const hovering = bubbleStyle('#365ba8', true, false, true)
  assert.equal(hovering.lineWidth, 0)
  assert.equal(hovering.stroke, 'transparent')
  assert.notDeepEqual(hovering.fill, resting.fill, 'hover still has a material feedback')
  const selected = bubbleStyle('#365ba8', true, true, true)
  assert.equal(selected.lineWidth, 0, 'review selection is also borderless')
  assert.notDeepEqual(selected.fill, resting.fill, 'review selection remains visible through the material')
})
