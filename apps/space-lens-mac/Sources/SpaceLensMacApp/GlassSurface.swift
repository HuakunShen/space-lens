import AppKit
import SwiftUI

struct GlassSurface<Content: View>: View {
    private let content: Content
    private let cornerRadius: CGFloat

    init(cornerRadius: CGFloat = 18, @ViewBuilder content: () -> Content) {
        self.cornerRadius = cornerRadius
        self.content = content()
    }

    var body: some View {
        Group {
            if #available(macOS 26.0, *) {
                content
                    .padding(14)
                    .glassEffect(.regular, in: .rect(cornerRadius: cornerRadius))
                    .overlay {
                        RoundedRectangle(cornerRadius: cornerRadius)
                            .strokeBorder(.white.opacity(0.08), lineWidth: 1)
                    }
            } else {
                content
                    .padding(14)
                    .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: cornerRadius))
                    .overlay {
                        RoundedRectangle(cornerRadius: cornerRadius)
                            .strokeBorder(.white.opacity(0.12), lineWidth: 1)
                    }
            }
        }
    }
}

struct GlassToolbar<Content: View>: View {
    private let content: Content

    init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    var body: some View {
        if #available(macOS 26.0, *) {
            GlassEffectContainer(spacing: 8) {
                content
                    .padding(.horizontal, 8)
                    .padding(.vertical, 6)
                    .glassEffect(.regular.interactive(), in: .capsule)
            }
        } else {
            content
                .padding(8)
                .background(.ultraThinMaterial, in: Capsule())
        }
    }
}

struct PhaseAwareGlassButtonStyle: ButtonStyle {
    let prominent: Bool
    @Environment(\.isEnabled) private var isEnabled
    @State private var isHovering = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.body.weight(.semibold))
            .foregroundStyle(prominent ? Color.white : Color.primary)
            .padding(.horizontal, 14)
            .padding(.vertical, 9)
            .background {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(backgroundColor(configuration: configuration))
            }
            .overlay {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .strokeBorder(borderColor, lineWidth: 1)
            }
            .contentShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .scaleEffect(configuration.isPressed ? 0.97 : 1)
            .opacity(isEnabled ? 1 : 0.48)
            .onHover { hovering in
                isHovering = hovering
                if isEnabled {
                    if hovering {
                        NSCursor.pointingHand.set()
                    } else {
                        NSCursor.arrow.set()
                    }
                }
            }
            .animation(.easeOut(duration: 0.14), value: isHovering)
            .animation(.easeOut(duration: 0.10), value: configuration.isPressed)
    }

    private func backgroundColor(configuration: Configuration) -> Color {
        guard isEnabled else { return .white.opacity(0.04) }
        if prominent {
            return Color.accentColor.opacity(configuration.isPressed ? 0.96 : isHovering ? 0.88 : 0.74)
        }
        return .white.opacity(configuration.isPressed ? 0.20 : isHovering ? 0.15 : 0.08)
    }

    private var borderColor: Color {
        .white.opacity(prominent ? (isHovering ? 0.34 : 0.20) : (isHovering ? 0.24 : 0.12))
    }
}
