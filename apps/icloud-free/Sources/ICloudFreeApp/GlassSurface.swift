import AppKit
import SwiftUI

struct WindowMaterialBackground: NSViewRepresentable {
    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = .underWindowBackground
        view.blendingMode = .behindWindow
        view.state = .active
        return view
    }

    func updateNSView(_ nsView: NSVisualEffectView, context: Context) {
        nsView.material = .underWindowBackground
        nsView.blendingMode = .behindWindow
        nsView.state = .active
    }
}

struct GlassSurface<Content: View>: View {
    private let content: Content
    private let tint: Color?

    init(tint: Color? = nil, @ViewBuilder content: () -> Content) {
        self.tint = tint
        self.content = content()
    }

    var body: some View {
        if #available(macOS 26.0, *) {
            content
                .padding(16)
                .glassEffect(.regular.tint(tint).interactive(), in: RoundedRectangle(cornerRadius: 20))
        } else {
            content
                .padding(16)
                .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 20))
                .overlay {
                    RoundedRectangle(cornerRadius: 20)
                        .strokeBorder(.white.opacity(0.12), lineWidth: 1)
                }
        }
    }
}
