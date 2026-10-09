import assert from 'node:assert/strict'
import { test } from 'node:test'
import { init, use } from 'echarts/core'
import { CustomChart, SunburstChart, TreemapChart } from 'echarts/charts'
import { SVGRenderer } from 'echarts/renderers'
import { UniversalTransition, LabelLayout } from 'echarts/features'
import { buildDiskChartModel } from '../src/lib/echarts-model.ts'
import { chartMaterial, chartSurfaceColor } from '../src/lib/chart-material.ts'
import { nodeColor, nodeMutedColor } from '../src/lib/colors.ts'
import { CHART_MODES } from '../src/lib/chart-mode.ts'
import type { ChartMode } from '../src/lib/chart-mode.ts'
import type { TreeSliceNode } from '../src/types.ts'

use([CustomChart, SunburstChart, TreemapChart, SVGRenderer, UniversalTransition, LabelLayout])

function node(
  id: string,
  size: number,
  children: TreeSliceNode[] = [],
  extra: Partial<TreeSliceNode> = {},
): TreeSliceNode {
  return {
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
    ...extra,
  }
}
const tree = node(
  'root',
  100,
  [node('folder', 70, [node('inner', 40, [], { depth: 2 }), node('other', 30, [], { depth: 2 })]), node('file', 30)],
  { depth: 0 },
)
const model = (mode: ChartMode, input: TreeSliceNode = tree) =>
  buildDiskChartModel({ tree: input, width: 720, height: 480, mode, density: 'nested', reducedMotion: true })

function whiteContrast(hex: string): number {
  const channels = [1, 3, 5]
    .map((start) => Number.parseInt(hex.slice(start, start + 2), 16) / 255)
    .map((value) => (value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4))
  const luminance = (channels[0] ?? 0) * 0.2126 + (channels[1] ?? 0) * 0.7152 + (channels[2] ?? 0) * 0.0722
  return 1.05 / (luminance + 0.05)
}

test('material guarantees white-label contrast at both stops across all hue families and depths', () => {
  for (let index = 0; index < 200; index++) {
    for (let depth = 1; depth <= 8; depth++) {
      for (const source of [nodeColor(`node-${index}`, depth), nodeMutedColor(depth)]) {
        for (const stop of chartMaterial(source).colorStops)
          assert.ok(whiteContrast(stop.color) >= 4.5, `${source} -> ${stop.color}`)
      }
    }
  }
  assert.equal(chartSurfaceColor('invalid'), '#41464f')
})

test('all layouts retain shared series and node identities for universal transitions', () => {
  const expected = ['file', 'folder', 'inner', 'other']
  for (const mode of CHART_MODES) {
    const result = model(mode)
    assert.deepEqual([...result.nodes.keys()].sort(), expected, mode)
    assert.deepEqual([...result.navigableIds].sort(), expected, mode)
    assert.ok(Array.isArray(result.option.series))
    const series = result.option.series[0]
    assert.equal(series?.id, 'disk')
    assert.deepEqual(series?.universalTransition, { enabled: true, seriesKey: 'disk', divideShape: 'clone' })
    assert.equal(result.option.animation, false)
    for (const [id, entry] of result.nodes) assert.equal(entry.node.id, id)
  }
})

test('native tree data indexes account for the virtual root and custom indexes start at zero', () => {
  assert.deepEqual(
    [...model('treemap').nodes].map(([id, entry]) => [id, entry.dataIndex]),
    [
      ['folder', 1],
      ['inner', 2],
      ['other', 3],
      ['file', 4],
    ],
  )
  assert.deepEqual(
    [...model('sunburst').nodes].map(([id, entry]) => [id, entry.dataIndex]),
    [
      ['folder', 1],
      ['inner', 2],
      ['other', 3],
      ['file', 4],
    ],
  )
  for (const mode of ['bubbles', 'strips', 'icicle'] satisfies ChartMode[]) {
    assert.deepEqual(
      [...model(mode).nodes.values()].map((entry) => entry.dataIndex),
      [0, 1, 2, 3],
    )
  }
})

test('omitted buckets appear once and cannot enter keyboard open targets', () => {
  const input = node(
    'root',
    100,
    [node('folder', 70, [], { hasChildren: true, childCount: 4, omittedBytes: 70, omittedCount: 4 }), node('file', 30)],
    { omittedBytes: 70, omittedCount: 4 },
  )
  for (const mode of CHART_MODES) {
    const result = model(mode, input)
    const aggregates = [...result.nodes.values()].filter((entry) => entry.isAggregate)
    assert.deepEqual(
      aggregates.map((entry) => [entry.node.id, entry.node.size]),
      [['folder:omitted', 70]],
      mode,
    )
    assert.ok(!result.navigableIds.includes('folder:omitted'))
  }
})

test('flat treemap preserves subtree weights without rendering descendants or counting parents twice', () => {
  const result = buildDiskChartModel({
    tree,
    width: 720,
    height: 480,
    mode: 'treemap',
    density: 'flat',
    reducedMotion: false,
  })
  assert.deepEqual([...result.nodes.keys()], ['folder', 'file'])
  assert.ok(Array.isArray(result.option.series))
  const series = result.option.series[0]
  assert.ok(series?.type === 'treemap')
  assert.deepEqual(
    series.data?.map((entry) => [entry.id, entry.value, entry.children]),
    [
      ['folder', 70, undefined],
      ['file', 30, undefined],
    ],
  )
  assert.equal(result.option.animation, true)
})

test('empty and zero-size canvases have no manufactured disk shapes', () => {
  for (const mode of CHART_MODES) {
    assert.equal(model(mode, node('empty', 0)).nodes.size, 0)
    assert.equal(
      buildDiskChartModel({ tree, width: 0, height: 480, mode, density: 'nested', reducedMotion: true }).nodes.size,
      0,
    )
  }
})

test('all five options render actual ECharts SVG shapes with safe text and no invalid coordinates', () => {
  for (const mode of CHART_MODES) {
    const chart = init(null, undefined, { renderer: 'svg', ssr: true, width: 720, height: 480 })
    try {
      chart.setOption(model(mode).option)
      const svg = chart.renderToSVGString()
      assert.match(svg, /<(path|circle|rect) /, mode)
      assert.doesNotMatch(svg, /NaN|Infinity|undefined/, mode)
      assert.match(svg, /folder/, mode)
      assert.match(svg, /#fff/, mode)
    } finally {
      chart.dispose()
    }
  }
})

test('nested treemap caps visible depth while preserving deeper measured weights', () => {
  const deep = node('root', 100, [
    node('folder', 100, [node('child', 100, [node('deep-file', 60), node('deep-file-2', 40)])]),
  ])
  const result = model('treemap', deep)
  assert.deepEqual([...result.nodes.keys()], ['folder', 'child'])
  assert.ok(Array.isArray(result.option.series))
  const series = result.option.series[0]
  assert.ok(series?.type === 'treemap')
  assert.equal(series.data?.[0]?.children?.[0]?.value, 100)
  assert.equal(series.data?.[0]?.children?.[0]?.children, undefined)
  assert.equal(series.levels?.[0]?.upperLabel?.show, false)
})

test('treemap parent headers retain contrast and family fill during collection and hover', () => {
  const result = buildDiskChartModel({
    tree,
    width: 720,
    height: 480,
    mode: 'treemap',
    density: 'nested',
    reducedMotion: true,
    collectedIds: new Set(['folder']),
  })
  assert.ok(Array.isArray(result.option.series))
  const series = result.option.series[0]
  assert.ok(series?.type === 'treemap')
  const parent = series.data?.[0]
  const header = parent?.itemStyle?.borderColor
  assert.equal(typeof header, 'string')
  assert.ok(typeof header === 'string' && whiteContrast(header) >= 4.5)
  assert.equal(parent?.emphasis?.itemStyle?.borderColor, header)
  assert.equal(parent?.emphasis?.itemStyle?.borderWidth, parent?.itemStyle?.borderWidth)
  const chart = init(null, undefined, { renderer: 'svg', ssr: true, width: 720, height: 480 })
  try {
    chart.setOption(result.option)
    assert.match(chart.renderToSVGString(), /✓ folder/)
    assert.doesNotMatch(chart.renderToSVGString(), />Disk usage</)
  } finally {
    chart.dispose()
  }
})
