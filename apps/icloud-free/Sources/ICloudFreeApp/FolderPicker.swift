import AppKit

enum FolderPicker {
    @MainActor
    static func chooseFolder() -> URL? {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = false
        panel.prompt = "Inspect"
        return panel.runModal() == .OK ? panel.url : nil
    }
}
