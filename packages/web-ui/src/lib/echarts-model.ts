import type { EChartsOption, CustomSeriesOption, TreemapSeriesOption, SunburstSeriesOption } from 'echarts'
type CustomSeriesRenderItemReturn = ReturnType<NonNullable<CustomSeriesOption['renderItem']>>
import type { TreeSliceNode } from '../types.ts'
import type { ChartMode, TreemapDensity } from './chart-mode.ts'
import { buildBubbleCircles, type BubbleCircle } from './bubbles.ts'
import { buildBubbleLabels, BUBBLE_FONT } from './bubble-label.ts'
import { buildIcicleSegments, clipIcicleName } from './icicle.ts'
import { buildStripRows } from './strips.ts'
import { withOmittedBuckets } from './sunburst.ts'
import { nodeColor, nodeMutedColor, sunburstColor } from './colors.ts'
import { bubbleStyle, chartMaterial, chartSurfaceColor } from './chart-material.ts'
import { SoftBubbleWorld, type SoftBubbleSnapshot } from './soft-bubbles/world.ts'
import { bubblePath } from './soft-bubbles/geometry.ts'
import { formatNodeSize } from './node-size.ts'

export interface DiskChartNode {
  node: TreeSliceNode
  color: string
  isAggregate: boolean
  childCount: number
  dataIndex: number
}

interface DiskChartInput {
  tree: TreeSliceNode
  width: number
  height: number
  mode: ChartMode
  density: TreemapDensity
  reducedMotion: boolean
  collectedIds?: Set<string>
  textColor?: string
  theme?: 'dark' | 'light'
}

export interface DiskChartModel {
  bubbles?: { circles: BubbleCircle[]; offset: number; rest: SoftBubbleSnapshot[] }
  option: EChartsOption
  nodes: Map<string, DiskChartNode>
  navigableIds: string[]
}

export const BUBBLE_CANVAS_INSET = 6

const bubbleLayouts = new WeakMap<TreeSliceNode, { width: number; height: number; layout: NonNullable<DiskChartModel['bubbles']> }>()

const FONT = '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
const EDGE = 'rgba(255,255,255,0.15)'
const EMPHASIS_EDGE = 'rgba(255,255,255,0.9)'

/** A single series identity lets ECharts match the same folder between layouts. */
export function buildDiskChartModel(input: DiskChartInput): DiskChartModel {
  const { tree, width, height, mode, density, reducedMotion, collectedIds = new Set<string>() } = input
  const nodes = new Map<string, DiskChartNode>()
  let bubbles: DiskChartModel['bubbles']
  const option: EChartsOption = {
    backgroundColor: 'transparent',
    animation: !reducedMotion,
    animationDuration: 300,
    animationDurationUpdate: 560,
    animationEasingUpdate: 'cubicInOut',
    textStyle: { fontFamily: FONT },
    series: [],
  }
  if (width <= 0 || height <= 0 || (tree.children.length === 0 && tree.omittedCount === 0)) {
    return { option, nodes, navigableIds: [] }
  }
  const common = {
    id: 'disk',
    name: 'Disk usage',
    universalTransition: { enabled: true, seriesKey: 'disk', divideShape: 'clone' as const },
    animationDurationUpdate: reducedMotion ? 0 : 560,
  }
  const textColor = input.textColor ?? '#e8e8e8'
  const root = withOmittedBuckets(tree)
  const sourceColors = new Map<string, string>()
  function register(node: TreeSliceNode, parentId: string, family: string, dataIndex: number) {
    const color = node.ignored
      ? nodeMutedColor(node.depth)
      : mode === 'sunburst'
        ? sunburstColor(node.id, node.depth)
        : nodeColor(node.id, node.depth, family)
    sourceColors.set(node.id, color)
    nodes.set(node.id, {
      node,
      color: mode === 'sunburst' ? color : chartSurfaceColor(color),
      isAggregate: node.id === `${parentId}:omitted`,
      childCount: node.childCount,
      dataIndex,
    })
  }
  const selectedEdge = (id: string) => (collectedIds.has(id) ? '#ffffff' : EDGE)
  const selectedWidth = (id: string) => (collectedIds.has(id) ? 2 : 1)
  if (mode === 'treemap' || mode === 'sunburst') {
    let dataIndex = 1 // ECharts inserts its virtual root before the preorder traversal.
    const data = root.children.map(function convert(node): NonNullable<TreemapSeriesOption['data']>[number] {
      return convertBranch(node, root.id, node.id)
    })
    function convertBranch(
      node: TreeSliceNode,
      parentId: string,
      family: string,
      visibleDepth = 1,
    ): NonNullable<TreemapSeriesOption['data']>[number] {
      register(node, parentId, family, dataIndex++)
      const children =
        mode === 'treemap' && (density === 'flat' || visibleDepth >= 2)
          ? []
          : node.children.map((child) => convertBranch(child, node.id, family, visibleDepth + 1))
      const source = sourceColors.get(node.id) ?? ''
      const parentTile = mode === 'treemap' && children.length > 0
      const borderColor = parentTile
        ? chartSurfaceColor(source)
        : mode === 'sunburst' && !collectedIds.has(node.id)
          ? '#101010'
          : selectedEdge(node.id)
      const name = `${collectedIds.has(node.id) ? '✓ ' : ''}${node.name}`
      return {
        id: node.id,
        name: node.name,
        value: leafValue(node),
        ...(children.length > 0 ? { children } : {}),
        itemStyle: {
          color: mode === 'sunburst' ? source : chartMaterial(source),
          borderColor,
          borderWidth: parentTile ? 4 : selectedWidth(node.id),
          borderRadius: mode === 'treemap' ? 7 : 0,
        },
        label: {
          show: mode !== 'sunburst',
          color: '#fff',
          formatter: () => (mode === 'treemap' ? `${name}\n${formatNodeSize(node)}` : name),
        },
        upperLabel: { color: '#fff', formatter: () => `${name}   ${formatNodeSize(node)}` },
        emphasis: {
          itemStyle: { borderColor: parentTile ? borderColor : EMPHASIS_EDGE, borderWidth: parentTile ? 4 : 2 },
          label: { show: mode !== 'sunburst', color: '#fff' },
        },
      }
    }
    if (mode === 'treemap') {
      const series: TreemapSeriesOption = {
        ...common,
        type: 'treemap',
        left: 4,
        right: 4,
        top: 4,
        bottom: 4,
        roam: false,
        nodeClick: false,
        breadcrumb: { show: false },
        sort: 'desc',
        visibleMin: 8,
        childrenVisibleMin: 350,
        label: {
          show: true,
          color: '#fff',
          position: 'insideTopLeft',
          padding: [12, 10],
          fontSize: 13,
          fontWeight: 500,
          lineHeight: 21,
          overflow: 'truncate',
          ellipsis: '…',
        },
        upperLabel: {
          show: density === 'nested',
          height: 38,
          color: '#fff',
          fontSize: 13,
          fontWeight: 600,
          padding: [0, 9],
          overflow: 'truncate',
        },
        itemStyle: { borderWidth: 4, gapWidth: 4, borderColor: 'transparent', borderRadius: 8 },
        levels: [
          { upperLabel: { show: false }, itemStyle: { borderWidth: 0, gapWidth: 5 } },
          { itemStyle: { borderWidth: 4, gapWidth: 4 } },
          { itemStyle: { borderWidth: 3, gapWidth: 3 } },
        ],
        data,
      }
      option.series = [series]
    } else {
      const outerRadius = Math.max(0, Math.min(width, height) / 2 - 10)
      const innerRadius = outerRadius * 0.24
      const depth = Math.max(1, relativeDepth(root))
      const ringWidth = (outerRadius - innerRadius) / depth
      const gap = Math.min(6, ringWidth * 0.12)
      const series: SunburstSeriesOption = {
        ...common,
        type: 'sunburst',
        center: ['50%', '50%'],
        radius: [innerRadius, outerRadius],
        nodeClick: false,
        sort: 'desc',
        label: { show: false },
        itemStyle: { borderColor: '#101010', borderWidth: 1, borderRadius: 0 },
        emphasis: { focus: 'none', label: { show: false }, itemStyle: { borderWidth: 2, borderColor: '#fff' } },
        levels: Array.from({ length: depth + 1 }, (_, level) =>
          level === 0
            ? {}
            : {
                r0: innerRadius + (level - 1) * ringWidth + gap / 2,
                r: innerRadius + level * ringWidth - gap / 2,
              },
        ),
        data,
      }
      option.series = [series]
    }
  } else {
    const shapes: {
      node: TreeSliceNode
      parentId: string
      family: string
      render: () => CustomSeriesRenderItemReturn
    }[] = []
    function style(id: string, color: string) {
      return { fill: chartMaterial(color), stroke: selectedEdge(id), lineWidth: selectedWidth(id) }
    }
    const emphasis = { style: { stroke: EMPHASIS_EDGE, lineWidth: 2 } }
    if (mode === 'bubbles') {
      const offset = BUBBLE_CANVAS_INSET
      let cached = bubbleLayouts.get(tree)
      if (!cached || cached.width !== width || cached.height !== height) {
        const circles = buildBubbleCircles(tree, width - offset * 2, height - offset * 2).filter((circle) => circle.id !== root.id)
        cached = { width, height, layout: { circles, offset, rest: new SoftBubbleWorld(circles, offset).compressedRest() } }
        bubbleLayouts.set(tree, cached)
      }
      bubbles = cached.layout
      const { circles, rest } = bubbles
      const restById = new Map(rest.map((body) => [body.id, body]))
      const labels = buildBubbleLabels(circles)
      const parentByChild = new Map<string, string>()
      const familyByChild = new Map<string, string>()
      walkParents(root, root.id, parentByChild, familyByChild)
      for (const circle of circles) {
        if (circle.id === root.id) continue
        const label = labels.get(circle.id)
        if (!label) continue
        const contour = restById.get(circle.id)!
        const layer = parentByChild.get(circle.id) === root.id ? 0 : 2
        const normalStyle = bubbleStyle(circle.color, label.hasVisibleChildren, collectedIds.has(circle.id), false, input.theme)
        shapes.push({
          node: circle.node,
          parentId: parentByChild.get(circle.id) ?? root.id,
          family: familyByChild.get(circle.id) ?? circle.id,
          render: () => ({
            type: 'path',
            id: circle.id,
            z2: layer,
            morph: true,
            shape: { pathData: bubblePath(contour.points) },
            style: normalStyle,
            emphasis: {
              z2: layer,
              style: bubbleStyle(circle.color, label.hasVisibleChildren, collectedIds.has(circle.id), true, input.theme),
            },
            textContent: {
              type: 'text',
              style: {
                text: label.text,
                x: label.x + contour.cx - circle.x,
                y: label.y + contour.cy - circle.y,
                fill: '#fff',
                fontFamily: BUBBLE_FONT,
                fontSize: label.fontSize,
                fontWeight: label.fontWeight,
                align: 'center',
                verticalAlign: 'middle',
                lineHeight: label.lineHeight,
              },
            },
            textConfig: { local: false, inside: true },
          }),
        })
      }
    } else if (mode === 'icicle') {
      const parentByChild = new Map<string, string>()
      const familyByChild = new Map<string, string>()
      walkParents(root, root.id, parentByChild, familyByChild)
      const depth = Math.min(4, Math.max(1, relativeDepth(root)))
      for (const segment of buildIcicleSegments(tree, width, height, depth)) {
        const w = Math.max(0, segment.x1 - segment.x0 - 4)
        const h = Math.max(0, segment.y1 - segment.y0 - 4)
        const label =
          segment.labelVisible && h >= 24
            ? `${clipIcicleName(segment.name, w)}${h >= 47 ? `\n${formatNodeSize(segment.node)}` : ''}`
            : ''
        shapes.push({
          node: segment.node,
          parentId: parentByChild.get(segment.id) ?? root.id,
          family: familyByChild.get(segment.id) ?? segment.id,
          render: () => ({
            type: 'rect',
            id: segment.id,
            morph: true,
            shape: { x: segment.x0 + 2, y: segment.y0 + 2, width: w, height: h, r: Math.min(6, h / 3) },
            style: style(segment.id, segment.color),
            emphasis,
            textContent: {
              type: 'text',
              style: { text: label, fill: '#fff', fontFamily: FONT, fontSize: 12, fontWeight: 500, lineHeight: 19 },
            },
            textConfig: { position: 'insideLeft', distance: 10, inside: true },
          }),
        })
      }
    } else {
      const rows = buildStripRows(tree)
      const rowHeight = Math.min(70, height / Math.max(1, rows.length))
      const largestShare = Math.max(...rows.map((row) => row.share), 0.001)
      rows.forEach((row, index) => {
        const y = index * rowHeight + 3
        const w = Math.max(1, ((width - 8) * row.share) / largestShare)
        const showHeading = rowHeight >= 39
        const barY = y + (showHeading ? 26 : 0)
        const h = Math.max(1, rowHeight - (showHeading ? 40 : 8))
        const metrics = `${formatNodeSize(row.node)}${row.node.scanState === 'skipped' ? '' : `  ·  ${Math.round(row.share * 100)}%`}`
        shapes.push({
          node: row.node,
          parentId: root.id,
          family: row.id,
          render: () => ({
            type: 'group',
            id: row.id,
            children: [
              {
                type: 'rect',
                name: 'bar',
                morph: true,
                shape: { x: 4, y: barY, width: w, height: h, r: Math.min(6, h / 3) },
                style: style(row.id, row.color),
                emphasis,
              },
              {
                type: 'text',
                name: 'name',
                style: {
                  text: showHeading ? row.name : '',
                  x: 5,
                  y: y + 5,
                  fill: textColor,
                  fontFamily: FONT,
                  fontSize: 13,
                  fontWeight: 500,
                  overflow: 'truncate',
                  width: Math.max(0, width - 170),
                },
              },
              {
                type: 'text',
                name: 'size',
                style: {
                  text: showHeading ? metrics : '',
                  x: width - 5,
                  y: y + 5,
                  fill: textColor,
                  fontFamily: FONT,
                  fontSize: 12,
                  align: 'right',
                },
              },
            ],
          }),
        })
        if (row.segments.length > 0 && h >= 12) {
          let x = 7
          row.segments.forEach((segment) => {
            const segmentX = x
            const segmentWidth = Math.max(0, w - 6) * segment.share
            x += segmentWidth
            if (segmentWidth < 3) return
            shapes.push({
              node: segment.node,
              parentId: row.id,
              family: row.id,
              render: () => ({
                type: 'rect',
                id: segment.id,
                morph: true,
                shape: {
                  x: segmentX,
                  y: barY + 3,
                  width: Math.max(1, segmentWidth - 3),
                  height: Math.max(1, h - 6),
                  r: 3,
                },
                style: style(segment.id, segment.color),
                emphasis,
                textContent: {
                  type: 'text',
                  style: {
                    text: segmentWidth > 80 && h >= 27 ? clipIcicleName(segment.name, segmentWidth) : '',
                    fill: '#fff',
                    fontFamily: FONT,
                    fontSize: 11,
                  },
                },
                textConfig: { position: 'insideLeft', distance: 7, inside: true },
              }),
            })
          })
        }
      })
    }
    shapes.forEach((shape, index) => register(shape.node, shape.parentId, shape.family, index))
    const series: CustomSeriesOption = {
      ...common,
      type: 'custom',
      coordinateSystem: 'none',
      clip: true,
      dimensions: ['value', 'id'],
      encode: { itemName: 1, itemId: 1 },
      data: shapes.map(({ node }) => ({ id: node.id, name: node.name, value: [leafValue(node), node.id] })),
      renderItem: (params) => shapes[params.dataIndex]?.render(),
    }
    option.series = [series]
  }
  return {
    option,
    nodes,
    bubbles,
    navigableIds: [...nodes].filter(([, entry]) => !entry.isAggregate).map(([id]) => id),
  }
}

function leafValue(node: TreeSliceNode): number {
  return node.children.length > 0
    ? node.children.reduce((sum, child) => sum + leafValue(child), 0)
    : Math.max(1, node.size)
}

function relativeDepth(node: TreeSliceNode): number {
  return node.children.length > 0 ? 1 + Math.max(...node.children.map(relativeDepth)) : 0
}

function walkParents(
  node: TreeSliceNode,
  family: string,
  parents: Map<string, string>,
  families: Map<string, string>,
  depth = 0,
): void {
  for (const child of node.children) {
    parents.set(child.id, node.id)
    const childFamily = depth === 0 ? child.id : family
    families.set(child.id, childFamily)
    walkParents(child, childFamily, parents, families, depth + 1)
  }
}
