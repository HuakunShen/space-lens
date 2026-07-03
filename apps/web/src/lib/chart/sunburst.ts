import { arc, hierarchy, partition } from "d3";
import type { HierarchyRectangularNode } from "d3";
import { nodeColor, nodeMutedColor } from "./colors";
import type { TreeSliceNode } from "$lib/api/types";

export interface SunburstSegment {
  id: string;
  name: string;
  path: string;
  size: number;
  depth: number;
  childCount: number;
  hasChildren: boolean;
  color: string;
  pathData: string;
  labelX: number;
  labelY: number;
  labelRotation: number;
  labelVisible: boolean;
  node: TreeSliceNode;
}

export function buildSunburstSegments(
  tree: TreeSliceNode,
  radius: number,
): SunburstSegment[] {
  const root = hierarchy(withOmittedBuckets(tree))
    .sum((node) => (node.children.length > 0 ? 0 : Math.max(1, node.size)))
    .sort((left, right) => (right.value ?? 0) - (left.value ?? 0));

  const laidOut = partition<TreeSliceNode>().size([Math.PI * 2, radius])(root);
  const makeArc = arc<HierarchyRectangularNode<TreeSliceNode>>()
    .startAngle((node) => node.x0)
    .endAngle((node) => node.x1)
    .innerRadius((node) => Math.max(62, node.y0 + 4))
    .outerRadius((node) => Math.max(64, node.y1 - 3))
    .cornerRadius(1)
    .padAngle(0.002);

  return laidOut
    .descendants()
    .filter((node) => node.depth > 0)
    .map((node, index) => {
      const middleAngle = (node.x0 + node.x1) / 2;
      const middleRadius = (node.y0 + node.y1) / 2;
      const degrees = (middleAngle * 180) / Math.PI - 90;
      return {
        id: node.data.id,
        name: node.data.name,
        path: node.data.path,
        size: node.data.size,
        depth: node.data.depth,
        childCount: node.data.childCount,
        hasChildren: node.data.hasChildren,
        color: node.data.ignored
          ? nodeMutedColor(node.depth)
          : nodeColor(node.data.id, node.depth, index),
        pathData: makeArc(node) ?? "",
        labelX: Math.cos(middleAngle - Math.PI / 2) * middleRadius,
        labelY: Math.sin(middleAngle - Math.PI / 2) * middleRadius,
        labelRotation: degrees > 90 ? degrees + 180 : degrees,
        labelVisible: node.x1 - node.x0 > 0.16 && node.y1 - node.y0 > 24,
        node: node.data,
      };
    });
}

function withOmittedBuckets(node: TreeSliceNode): TreeSliceNode {
  const children = node.children.map(withOmittedBuckets);
  if (node.omittedBytes > 0) {
    children.push({
      id: `${node.id}:omitted`,
      name: "Other",
      path: node.path,
      size: node.omittedBytes,
      depth: node.depth + 1,
      ignored: false,
      hasChildren: false,
      childCount: node.omittedCount,
      loadedDepth: 0,
      truncated: false,
      children: [],
      omittedBytes: 0,
      omittedCount: 0,
    });
  }
  return {
    ...node,
    children,
  };
}
