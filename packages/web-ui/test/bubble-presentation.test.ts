/** The Canvas handoff uses exactly the circles, labels, and gradients painted by ECharts. */
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { graphic, init, use } from 'echarts/core'
import { CustomChart } from 'echarts/charts'
import { SVGRenderer } from 'echarts/renderers'
import { buildDiskChartModel, BUBBLE_CANVAS_INSET } from '../src/lib/echarts-model.ts'
import { buildBubbleLabels } from '../src/lib/bubble-label.ts'
import { bubbleStyle, gradientInBounds } from '../src/lib/chart-material.ts'
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

test('exposed bubble seeds match every actual ECharts circle and share label decisions', () => {
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
  assert.equal(labels.get('folder')?.hasVisibleChildren, true)
  assert.equal(labels.get('nested')?.hasVisibleChildren, false)
  const chart = init(null, undefined, { renderer: 'svg', ssr: true, width: 720, height: 480 })
  try {
    chart.setOption(model.option)
    const drawn = chart
      .getZr()
      .storage.getDisplayList(true)
      .filter((element) => element instanceof graphic.Circle)
    assert.equal(drawn.length, model.bubbles.circles.length)
    for (const seed of model.bubbles.circles) {
      assert.ok(
        drawn.some(
          (element) =>
            element.shape.cx === seed.x + model.bubbles.offset &&
            element.shape.cy === seed.y + model.bubbles.offset &&
            element.shape.r === seed.r,
        ),
      )
    }
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
  assert.equal(bubbleStyle('#365ba8', true, false, true).stroke, 'rgba(255,255,255,0.9)')
  assert.equal(bubbleStyle('#365ba8', true, true).lineWidth, 2)
})
