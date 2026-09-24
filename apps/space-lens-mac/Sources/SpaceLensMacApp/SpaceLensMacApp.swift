import SwiftUI
import AppKit

@main
struct SpaceLensMacApp: App {
    @NSApplicationDelegateAdaptor(SpaceLensAppDelegate.self) private var appDelegate

    var body: some Scene {
        WindowGroup("Space Lens") {
            ContentView()
        }
        .windowStyle(.hiddenTitleBar)
        .windowToolbarStyle(.unifiedCompact)
        .commands {
            CommandGroup(after: .textEditing) {
                Button("Open Space Map") {}
                    .keyboardShortcut("o", modifiers: [.command, .shift])
            }
        }
    }
}

final class SpaceLensAppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        let bundles = [Bundle.module, Bundle.main]
        let iconURLs = bundles.flatMap { bundle in
            [
                bundle.url(forResource: "SpaceLens", withExtension: "icns"),
                bundle.url(forResource: "SpaceLensLogo", withExtension: "png")
            ].compactMap { $0 }
        }
        if let imageURL = iconURLs.first,
           let image = NSImage(contentsOf: imageURL) {
            NSApplication.shared.applicationIconImage = image
        }
    }
}
