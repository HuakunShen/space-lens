import SwiftUI

struct SunburstSegment: Identifiable, Hashable {
    let id: String
    let nodeID: String
    let name: String
    let depth: Int
    let startAngle: Double
    let endAngle: Double
    let innerRadius: CGFloat
    let outerRadius: CGFloat
    let hue: Double
    let isOther: Bool
    let allocatedBytes: UInt64
    let logicalBytes: UInt64

    var span: Double { endAngle - startAngle }

    var color: Color {
        Color(hue: hue, saturation: depth == 0 ? 0.68 : 0.52, brightness: depth == 0 ? 0.88 : 0.82)
    }
}

enum SunburstLayout {
    static func segments(
        for root: SpaceLensNode,
        maxDepth: Int = 4,
        maxChildrenPerNode: Int = 24,
        innerRadius: CGFloat = 0.18,
        outerRadius: CGFloat = 0.96
    ) -> [SunburstSegment] {
        guard !root.children.isEmpty, maxDepth > 0 else { return [] }

        let sorted = root.children.sorted { lhs, rhs in
            lhs.allocatedBytes == rhs.allocatedBytes ? lhs.name < rhs.name : lhs.allocatedBytes > rhs.allocatedBytes
        }
        return layoutChildren(
            sorted,
            parentTotal: max(root.allocatedBytes, 1),
            startAngle: -.pi / 2,
            endAngle: 3 * .pi / 2,
            depth: 0,
            maxDepth: maxDepth,
            maxChildrenPerNode: maxChildrenPerNode,
            innerRadius: innerRadius,
            outerRadius: outerRadius
        )
    }

    private static func layoutChildren(
        _ children: [SpaceLensNode],
        parentTotal: UInt64,
        startAngle: Double,
        endAngle: Double,
        depth: Int,
        maxDepth: Int,
        maxChildrenPerNode: Int,
        innerRadius: CGFloat,
        outerRadius: CGFloat
    ) -> [SunburstSegment] {
        guard !children.isEmpty else { return [] }
        let visible = Array(children.prefix(maxChildrenPerNode))
        let omittedBytes = children.dropFirst(maxChildrenPerNode).reduce(UInt64.zero) { $0 + $1.allocatedBytes }
        let displayChildren = visible + (omittedBytes > 0 ? [SpaceLensNode(
            id: "other-" + String(depth) + "-" + String(startAngle),
            name: "Other",
            kind: .other,
            allocatedBytes: omittedBytes,
            logicalBytes: omittedBytes,
            children: []
        )] : [])
        let total = max(displayChildren.reduce(UInt64.zero) { $0 + $1.allocatedBytes }, 1)
        let ringWidth = (outerRadius - innerRadius) / CGFloat(max(maxDepth, 1))
        var cursor = startAngle
        var result: [SunburstSegment] = []

        for (index, child) in displayChildren.enumerated() {
            let share = Double(child.allocatedBytes) / Double(total)
            let next = index == displayChildren.count - 1
                ? endAngle
                : cursor + (endAngle - startAngle) * share
            let segment = SunburstSegment(
                id: child.id,
                nodeID: child.id,
                name: child.name,
                depth: depth,
                startAngle: cursor,
                endAngle: next,
                innerRadius: innerRadius + CGFloat(depth) * ringWidth,
                outerRadius: innerRadius + CGFloat(depth + 1) * ringWidth,
                hue: colorHue(for: index, depth: depth),
                isOther: child.name == "Other",
                allocatedBytes: child.allocatedBytes,
                logicalBytes: child.logicalBytes
            )
            result.append(segment)

            if depth + 1 < maxDepth, !child.children.isEmpty {
                result.append(contentsOf: layoutChildren(
                    child.children.sorted { lhs, rhs in
                        lhs.allocatedBytes == rhs.allocatedBytes ? lhs.name < rhs.name : lhs.allocatedBytes > rhs.allocatedBytes
                    },
                    parentTotal: child.allocatedBytes,
                    startAngle: cursor,
                    endAngle: next,
                    depth: depth + 1,
                    maxDepth: maxDepth,
                    maxChildrenPerNode: maxChildrenPerNode,
                    innerRadius: innerRadius,
                    outerRadius: outerRadius
                ))
            }
            cursor = next
        }
        _ = parentTotal
        return result
    }

    private static func colorHue(for index: Int, depth: Int) -> Double {
        let palette = [0.54, 0.62, 0.70, 0.78, 0.87, 0.34, 0.25, 0.16, 0.46]
        let base = palette[index % palette.count]
        return (base + Double(depth) * 0.018).truncatingRemainder(dividingBy: 1.0)
    }
}
