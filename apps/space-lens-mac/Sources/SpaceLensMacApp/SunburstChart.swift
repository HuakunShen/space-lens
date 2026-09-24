import SwiftUI

struct SunburstChart: View {
    let root: SpaceLensNode
    @Binding var selectedNodeID: String?
    @State private var hoveredSegmentID: String?

    private var segments: [SunburstSegment] {
        SunburstLayout.segments(for: root)
    }

    var body: some View {
        GeometryReader { proxy in
            let diameter = min(proxy.size.width, proxy.size.height)
            let center = CGPoint(x: proxy.size.width / 2, y: proxy.size.height / 2)
            let radius = diameter / 2

            ZStack {
                Canvas { context, size in
                    let canvasCenter = CGPoint(x: size.width / 2, y: size.height / 2)
                    let canvasRadius = diameter / 2
                    for segment in segments {
                        let path = segmentPath(segment, center: canvasCenter, radius: canvasRadius)
                        let isSelected = selectedNodeID == segment.nodeID
                        let isHovered = hoveredSegmentID == segment.id
                        context.fill(path, with: .color(segment.color.opacity(isSelected || isHovered ? 1.0 : 0.9)))
                        context.stroke(
                            path,
                            with: .color(.black.opacity(isSelected || isHovered ? 0.55 : 0.2)),
                            lineWidth: isSelected || isHovered ? 2.2 : 0.8
                        )
                    }
                }
                .accessibilityLabel("Space usage map for \(root.name)")

                // Keep the chart in one Canvas, but use transparent SwiftUI
                // hit shapes so AppKit/SwiftUI hover works for every segment.
                ForEach(segments) { segment in
                    let path = segmentPath(segment, center: center, radius: radius)
                    path
                        .fill(.clear)
                        .contentShape(path)
                        .onHover { isHovering in
                            if isHovering {
                                hoveredSegmentID = segment.id
                            } else if hoveredSegmentID == segment.id {
                                hoveredSegmentID = nil
                            }
                        }
                        .zIndex(1)
                }

                Button {
                    selectedNodeID = root.id
                } label: {
                    VStack(spacing: 3) {
                        Text(root.name)
                            .font(.headline)
                            .lineLimit(1)
                        Text(ByteCountFormatter.string(fromByteCount: Int64(root.allocatedBytes), countStyle: .file))
                            .font(.system(.title2, design: .rounded).weight(.bold))
                            .monospacedDigit()
                        Text("allocated")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    .frame(width: diameter * 0.24, height: diameter * 0.24)
                    .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .accessibilityHint("Select the current root")
                .zIndex(2)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .overlay {
                if let hoveredSegment {
                    SunburstTooltip(segment: hoveredSegment, root: root)
                        .position(tooltipPosition(for: hoveredSegment, in: proxy.size, diameter: diameter))
                        .allowsHitTesting(false)
                        .transition(.opacity)
                }
            }
            .animation(.easeOut(duration: 0.12), value: hoveredSegmentID)
        }
        .aspectRatio(1, contentMode: .fit)
        .padding(20)
    }

    private var hoveredSegment: SunburstSegment? {
        guard let hoveredSegmentID else { return nil }
        return segments.first { $0.id == hoveredSegmentID }
    }

    private func tooltipPosition(for segment: SunburstSegment, in size: CGSize, diameter: CGFloat) -> CGPoint {
        let estimatedWidth: CGFloat = 220
        let estimatedHeight: CGFloat = 82
        let center = CGPoint(x: size.width / 2, y: size.height / 2)
        let midAngle = (segment.startAngle + segment.endAngle) / 2
        let midRadius = (segment.innerRadius + segment.outerRadius) / 2 * diameter / 2
        let anchor = CGPoint(
            x: center.x + cos(midAngle) * midRadius,
            y: center.y + sin(midAngle) * midRadius
        )
        let preferredY = anchor.y < size.height / 2
            ? anchor.y + estimatedHeight / 2 + 16
            : anchor.y - estimatedHeight / 2 - 16
        let x = min(max(anchor.x, estimatedWidth / 2 + 8), size.width - estimatedWidth / 2 - 8)
        let y = min(max(preferredY, estimatedHeight / 2 + 8), size.height - estimatedHeight / 2 - 8)
        return CGPoint(x: x, y: y)
    }

    private func segmentPath(_ segment: SunburstSegment, center: CGPoint, radius: CGFloat) -> Path {
        let inner = radius * segment.innerRadius
        let outer = radius * segment.outerRadius
        let start = Angle(radians: segment.startAngle)
        let end = Angle(radians: segment.endAngle)
        let startPoint = CGPoint(
            x: center.x + cos(segment.startAngle) * inner,
            y: center.y + sin(segment.startAngle) * inner
        )
        var path = Path()
        path.move(to: startPoint)
        path.addArc(center: center, radius: outer, startAngle: start, endAngle: end, clockwise: false)
        path.addLine(to: CGPoint(
            x: center.x + cos(segment.endAngle) * inner,
            y: center.y + sin(segment.endAngle) * inner
        ))
        path.addArc(center: center, radius: inner, startAngle: end, endAngle: start, clockwise: true)
        path.closeSubpath()
        return path
    }
}

private struct SunburstTooltip: View {
    let segment: SunburstSegment
    let root: SpaceLensNode

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(segment.name)
                .font(.headline)
                .lineLimit(1)
            HStack(spacing: 8) {
                Text(ByteCountFormatter.string(fromByteCount: Int64(segment.allocatedBytes), countStyle: .file))
                    .font(.subheadline.monospacedDigit().weight(.semibold))
                Text(percentageOfRoot)
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
            Text(segment.isOther ? "Aggregated smaller items" : "Level \(segment.depth + 1) · hover for details")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(11)
        .frame(width: 220, alignment: .leading)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 13, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 13, style: .continuous)
                .strokeBorder(.white.opacity(0.16), lineWidth: 1)
        }
        .shadow(color: .black.opacity(0.18), radius: 12, y: 5)
    }

    private var percentageOfRoot: String {
        guard root.allocatedBytes > 0 else { return "0% of root" }
        let percentage = Double(segment.allocatedBytes) / Double(root.allocatedBytes) * 100
        return "\(percentage.formatted(.number.precision(.fractionLength(percentage < 1 ? 1 : 0))))% of root"
    }
}
