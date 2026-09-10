import AppKit
import ICloudFreeAppSupport
import SwiftUI

@main
struct ICloudFreeApp: App {
    @StateObject private var model = AppModel()

    init() {
        if let url = Bundle.module.url(forResource: "SpaceLensLogo", withExtension: "png"),
           let icon = NSImage(contentsOf: url) {
            NSApplication.shared.applicationIconImage = icon
        }
    }

    var body: some Scene {
        WindowGroup {
            ContentView(model: model)
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 1120, height: 760)
    }
}
