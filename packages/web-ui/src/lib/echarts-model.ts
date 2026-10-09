import type { EChartsOption, CustomSeriesOption, TreemapSeriesOption, SunburstSeriesOption } from 'echarts'
type CustomSeriesRenderItemReturn = ReturnType<NonNullable<CustomSeriesOption['renderItem']>>
import type { TreeSliceNode } from '../types.ts'
import type { ChartMode, TreemapDensity } from './chart-mode.ts'
import { buildBubbleCircles, clipBubbleName } from './bubbles.ts'
import { buildIcicleSegments, clipIcicleName } from './icicle.ts'
import { buildStripRows } from './strips.ts'
import { withOmittedBuckets } from './sunburst.ts'
import { nodeColor, nodeMutedColor } from './colors.ts'
import { bubbleMaterial, chartMaterial, chartSurfaceColor } from './chart-material.ts'
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
}

interface DiskChartModel {
  option: EChartsOption
  nodes: Map<string, DiskChartNode>
  navigableIds: string[]
}

const FONT = '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
const EDGE = 'rgba(255,255,255,0.15)'
const EMPHASIS_EDGE = 'rgba(255,255,255,0.9)'

/** A single series identity lets ECharts match the same folder between layouts. */
export function buildDiskChartModel(input: DiskChartInput): DiskChartModel {
  const { tree, width, height, mode, density, reducedMotion, collectedIds = new Set<string>() } = input
  const nodes = new Map<string, DiskChartNode>()
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
    const color = node.ignored ? nodeMutedColor(node.depth) : nodeColor(node.id, node.depth, family)
    sourceColors.set(node.id, color)
    nodes.set(node.id, {
      node,
      color: chartSurfaceColor(color),
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
      const borderColor = parentTile ? chartSurfaceColor(source) : selectedEdge(node.id)
      const name = `${collectedIds.has(node.id) ? '✓ ' : ''}${node.name}`
      return {
        id: node.id,
        name: node.name,
        value: leafValue(node),
        ...(children.length > 0 ? { children } : {}),
        itemStyle: {
          color: chartMaterial(source),
          borderColor,
          borderWidth: parentTile ? 4 : selectedWidth(node.id),
          borderRadius: mode === 'treemap' ? 7 : 3,
        },
        label: {
          color: '#fff',
          formatter: () => (mode === 'treemap' ? `${name}\n${formatNodeSize(node)}` : name),
        },
        upperLabel: { color: '#fff', formatter: () => `${name}   ${formatNodeSize(node)}` },
        emphasis: {
          itemStyle: { borderColor: parentTile ? borderColor : EMPHASIS_EDGE, borderWidth: parentTile ? 4 : 2 },
          label: { color: '#fff' },
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
      const series: SunburstSeriesOption = {
        ...common,
        type: 'sunburst',
        center: ['50%', '50%'],
        radius: [Math.min(width, height) * 0.12, '94%'],
        nodeClick: false,
        sort: 'desc',
        label: { show: true, color: '#fff', rotate: 'tangential', fontSize: 12, minAngle: 13, overflow: 'truncate' },
        labelLayout: { hideOverlap: true },
        itemStyle: { borderColor: '#202020', borderWidth: 3, borderRadius: 3 },
        emphasis: { focus: 'none', itemStyle: { borderWidth: 2, borderColor: '#fff' } },
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
      const circles = buildBubbleCircles(tree, width - 12, height - 12)
      const parentByChild = new Map<string, string>()
      const familyByChild = new Map<string, string>()
      walkParents(root, root.id, parentByChild, familyByChild)
      for (const circle of circles) {
        if (circle.id === root.id) continue
        const children = circles.filter((child) => parentByChild.get(child.id) === circle.id)
        const hasVisibleChildren = children.length > 0
        // ECharts raises emphasis by default; a packed parent must stay behind its children.
        const layer = parentByChild.get(circle.id) === root.id ? 0 : 2
        const band = hasVisibleChildren
          ? Math.min(...children.map((child) => child.y - child.r)) - (circle.y - circle.r)
          : circle.r * 2
        const titleY = hasVisibleChildren ? circle.y - circle.r + band / 2 : circle.y
        const chord = Math.sqrt(Math.max(0, circle.r ** 2 - (Math.abs(titleY - circle.y) + 8) ** 2))
        const name = clipBubbleName(circle.name, chord)
        const label =
          circle.labelVisible && circle.r >= 27 && band >= 22
            ? `${name}${circle.r >= 40 && band >= 40 ? `\n${formatNodeSize(circle.node)}` : ''}`
            : ''
        const fill = bubbleMaterial(circle.color, hasVisibleChildren)
        shapes.push({
          node: circle.node,
          parentId: parentByChild.get(circle.id) ?? root.id,
          family: familyByChild.get(circle.id) ?? circle.id,
          render: () => ({
            type: 'circle',
            id: circle.id,
            z2: layer,
            morph: true,
            shape: { cx: circle.x + 6, cy: circle.y + 6, r: circle.r },
            style: {
              fill,
              stroke: collectedIds.has(circle.id)
                ? '#fff'
                : hasVisibleChildren
                  ? 'rgba(255,255,255,0.16)'
                  : 'rgba(255,255,255,0.3)',
              lineWidth: selectedWidth(circle.id),
            },
            emphasis: { z2: layer, style: { fill, stroke: EMPHASIS_EDGE, lineWidth: 1.5 } },
            textContent: {
              type: 'text',
              style: {
                text: label,
                x: circle.x + 6,
                y: titleY + 6,
                fill: '#fff',
                fontFamily: FONT,
                fontSize: circle.r > 90 ? 14 : 12,
                fontWeight: hasVisibleChildren ? 600 : 500,
                align: 'center',
                verticalAlign: 'middle',
                lineHeight: 19,
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
  return { option, nodes, navigableIds: [...nodes].filter(([, entry]) => !entry.isAggregate).map(([id]) => id) }
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
