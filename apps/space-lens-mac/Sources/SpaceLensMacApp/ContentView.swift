import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @State private var section: SidebarSection? = .home
    @State private var selectedNodeID: String?
    @State private var root = SpaceLensNode.demoRoot
    @State private var snapshotJSON: Data?
    @State private var scannedPath: String?
    @State private var isScanning = false
    @State private var collector = CleanupCollector()
    @State private var isCleanupReviewPresented = false
    @State private var isCleaning = false
    @State private var cleanupProgress: TrashCleanupProgress?
    @State private var scanError: String?

    var body: some View {
        NavigationSplitView {
            sidebar
        } detail: {
            detail
        }
        .navigationSplitViewStyle(.balanced)
        .frame(minWidth: 860, minHeight: 620)
        .alert("Couldn’t complete operation", isPresented: Binding(
            get: { scanError != nil },
            set: { if !$0 { scanError = nil } }
        )) {
            Button("OK") { scanError = nil }
        } message: {
            Text(scanError ?? "Unknown scanner error")
        }
        .sheet(isPresented: $isCleanupReviewPresented) {
            CleanupReviewView(
                entries: collector.entries,
                isProcessing: isCleaning,
                progress: cleanupProgress,
                onRemove: { entry in
                    collector.remove(id: entry.id)
                },
                onMoveToTrash: {
                    beginTrashCleanup()
                }
            )
        }
    }

    private var sidebar: some View {
        SpaceLensSidebar(selection: $section, hasScannedFolder: scannedPath != nil)
    }

    @ViewBuilder
    private var detail: some View {
        switch section ?? .home {
        case .home:
            HomeView(
                openMap: {
                    section = .map
                    selectedNodeID = root.id
                },
                scanFolder: {
                    chooseFolder()
                },
                isScanning: isScanning
            )
        case .map:
            mapView
        case .recent, .collections, .exports:
            PlaceholderView(section: section ?? .home) {
                section = .home
            }
        case .iCloud:
            ICloudLocalCopiesView()
        }
    }

    private var mapView: some View {
        VStack(spacing: 12) {
            BreadcrumbBar {
                HStack(spacing: 12) {
                    Button {
                        section = .home
                    } label: {
                        Image(systemName: "chevron.left")
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Back to Home")

                    Divider().frame(height: 18)

                    Label("Disks and Folders", systemImage: "externaldrive")
                    Image(systemName: "chevron.right")
                        .foregroundStyle(.tertiary)
                    Text(root.name)
                        .fontWeight(.semibold)
                    Spacer()
                    Text(scannedPath == nil ? "Synthetic preview" : "Read-only snapshot")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    if snapshotJSON != nil {
                        Button {
                            exportSnapshot()
                        } label: {
                            Label("Export", systemImage: "square.and.arrow.up")
                        }
                        .buttonStyle(PhaseAwareGlassButtonStyle(prominent: false))
                    }
                }
            }

            HStack(alignment: .top, spacing: 12) {
                GlassSurface(cornerRadius: 22) {
                    SunburstChart(root: root, selectedNodeID: $selectedNodeID)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)

                InspectorView(
                    root: root,
                    selectedNodeID: $selectedNodeID,
                    onAddToCollector: addToCollector
                )
                    .frame(minWidth: 260, idealWidth: 320, maxWidth: 350)
            }

            GlassSurface(cornerRadius: 18) {
                HStack(spacing: 12) {
                    Image(systemName: "tray.full.fill")
                        .foregroundStyle(.tint)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Collector")
                            .font(.headline)
                        Text(collector.entries.isEmpty
                            ? "Select an item in the map or list to stage a cleanup plan."
                            : "Review the staged items before moving them to Trash.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Text("\(collector.entries.count) items · \(formattedBytes(collector.totalBytes))")
                        .font(.subheadline.monospacedDigit())
                        .foregroundStyle(.secondary)
                    Button("Review Cleanup") {
                        isCleanupReviewPresented = true
                    }
                        .buttonStyle(PhaseAwareGlassButtonStyle(prominent: false))
                        .disabled(collector.entries.isEmpty || isCleaning)
                }
            }
        }
        .padding(14)
        .background(SpaceLensBackground())
    }
}

private struct SpaceLensSidebar: View {
    @Binding var selection: SidebarSection?
    let hasScannedFolder: Bool

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                HStack(spacing: 9) {
                    Image(systemName: "circle.grid.3x3.fill")
                        .font(.title3.weight(.semibold))
                        .foregroundStyle(.tint)
                        .frame(width: 22)
                    Text("Space Lens")
                        .font(.headline)
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, 10)
                .padding(.top, 8)

                sidebarGroup("Lens", items: [.home, .map])
                sidebarGroup("Organize", items: [.recent, .collections, .exports])
                sidebarGroup("Capabilities", items: [.iCloud])
            }
            .padding(.horizontal, 12)
            .padding(.bottom, 16)
        }
        .scrollIndicators(.hidden)
        .frame(minWidth: 190, idealWidth: 220, maxWidth: 280)
        .navigationSplitViewColumnWidth(min: 190, ideal: 220, max: 280)
        .background(.regularMaterial)
        .overlay(alignment: .trailing) {
            Rectangle()
                .fill(.white.opacity(0.08))
                .frame(width: 1)
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            HStack(spacing: 8) {
                Image(systemName: "sparkles")
                    .foregroundStyle(.tint)
                Text(hasScannedFolder ? "Read-only folder scan" : "Phase 0 · synthetic map")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)
            .background(.regularMaterial)
        }
    }

    @ViewBuilder
    private func sidebarGroup(_ title: String, items: [SidebarSection], disabled: Bool = false) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title.uppercased())
                .font(.caption2.weight(.semibold))
                .tracking(0.7)
                .foregroundStyle(.secondary)
                .padding(.horizontal, 10)

            ForEach(items, id: \.self) { item in
                Button {
                    guard !disabled else { return }
                    selection = item
                } label: {
                    Label(item.title, systemImage: item.icon)
                        .font(.body.weight(selection == item ? .semibold : .regular))
                        .foregroundStyle(selection == item ? Color.accentColor : .primary)
                        .symbolVariant(selection == item ? .fill : .none)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 7)
                        .contentShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                }
                .buttonStyle(.plain)
                .background {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(selection == item ? Color.accentColor.opacity(0.13) : .clear)
                }
                .opacity(disabled ? 0.48 : 1)
                .accessibilityAddTraits(selection == item ? .isSelected : [])
            }
        }
    }
}

private struct BreadcrumbBar<Content: View>: View {
    private let content: Content

    init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    var body: some View {
        content
            .frame(minHeight: 48)
            .padding(.horizontal, 14)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(.white.opacity(0.10), lineWidth: 1)
            }
    }
}

private struct HomeView: View {
    let openMap: () -> Void
    let scanFolder: () -> Void
    let isScanning: Bool

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                HStack(alignment: .firstTextBaseline) {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Understand your space")
                            .font(.system(.largeTitle, design: .rounded).weight(.bold))
                        Text("See the shape of your storage before you decide what to keep.")
                            .font(.title3)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Image(systemName: "circle.grid.3x3.fill")
                        .font(.system(size: 52, weight: .medium))
                        .foregroundStyle(.tint)
                        .symbolRenderingMode(.hierarchical)
                }

                GlassSurface(cornerRadius: 22) {
                    VStack(alignment: .leading, spacing: 18) {
                        Label("Synthetic preview", systemImage: "sparkles")
                            .font(.headline)
                            .foregroundStyle(.tint)
                        Text("Explore the Phase 0 Sunburst prototype")
                            .font(.title2.weight(.semibold))
                        Text("This preview uses a deterministic filesystem fixture. Scan a real folder to inspect it without modifying anything, or open the demo map to explore the layout.")
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                        HStack(spacing: 10) {
                            Button(action: scanFolder) {
                                Label("Scan a Folder", systemImage: "folder.badge.plus")
                            }
                            .buttonStyle(PhaseAwareGlassButtonStyle(prominent: true))

                            Button(action: openMap) {
                                Label("Open Demo Map", systemImage: "arrow.right.circle")
                            }
                            .buttonStyle(PhaseAwareGlassButtonStyle(prominent: false))
                        }
                        if isScanning {
                            HStack(spacing: 8) {
                                ProgressView()
                                    .controlSize(.small)
                                Text("Scanning…")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }

                HStack(spacing: 14) {
                    volumeCard("Macintosh HD", "690 GB", "76% used", "internaldrive.fill", .cyan)
                    volumeCard("External Drive", "2 TB", "Not scanned", "externaldrive.fill", .purple)
                }
            }
            .padding(32)
            .frame(maxWidth: 980, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .background(SpaceLensBackground())
    }

    private func volumeCard(_ name: String, _ size: String, _ detail: String, _ icon: String, _ color: Color) -> some View {
        GlassSurface(cornerRadius: 18) {
            HStack(spacing: 12) {
                Image(systemName: icon)
                    .font(.title2)
                    .foregroundStyle(color)
                VStack(alignment: .leading, spacing: 4) {
                    Text(name).font(.headline)
                    Text(size).font(.title3.monospacedDigit())
                    Text(detail).font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
            }
        }
        .frame(maxWidth: .infinity)
    }
}

private extension ContentView {
    func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = false
        panel.allowsMultipleSelection = false
        panel.prompt = "Scan Folder"
        guard panel.runModal() == .OK, let url = panel.url else { return }

        collector.removeAll()
        scanFolder(at: url)
    }

    func scanFolder(at url: URL) {
        isScanning = true
        scanError = nil
        let normalizedURL = url.standardizedFileURL
        Task.detached(priority: .userInitiated) {
            do {
                let result = try RustScanClient.scan(path: normalizedURL.path)
                await MainActor.run {
                    root = result.root
                    snapshotJSON = result.json
                    scannedPath = normalizedURL.path
                    selectedNodeID = result.root.id
                    section = .map
                    isScanning = false
                }
            } catch {
                await MainActor.run {
                    scanError = error.localizedDescription
                    isScanning = false
                }
            }
        }
    }

    func addToCollector(_ node: SpaceLensNode) {
        guard scannedPath != nil,
              node.id != root.id,
              node.sourceURL != URL(fileURLWithPath: scannedPath ?? "") else {
            return
        }
        _ = collector.add(node: node)
        selectedNodeID = node.id
    }

    func beginTrashCleanup() {
        guard !isCleaning, !collector.entries.isEmpty else { return }

        isCleaning = true
        cleanupProgress = TrashCleanupProgress(
            processed: 0,
            total: collector.entries.count,
            moved: 0,
            failed: 0,
            currentName: nil
        )
        scanError = nil
        let entries = collector.entries

        Task.detached(priority: .userInitiated) {
            let report = TrashCleanupService().moveToTrash(entries) { progress in
                Task { @MainActor in
                    cleanupProgress = progress
                }
            }

            await MainActor.run {
                let movedIDs = Set(report.moved.map(\.id))
                collector.remove(ids: movedIDs)
                isCleaning = false

                if !report.failures.isEmpty {
                    let details = report.failures
                        .map { "\($0.name): \($0.message)" }
                        .joined(separator: "\n")
                    scanError = "Some items could not be moved to Trash.\n\(details)"
                }

                if collector.entries.isEmpty {
                    isCleanupReviewPresented = false
                }

                if !report.moved.isEmpty,
                   let scannedPath,
                   FileManager.default.fileExists(atPath: scannedPath) {
                    scanFolder(at: URL(fileURLWithPath: scannedPath, isDirectory: true))
                }
            }
        }
    }

    func exportSnapshot() {
        guard let snapshotJSON else { return }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.json]
        panel.nameFieldStringValue = "space-lens-snapshot.json"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try snapshotJSON.write(to: url, options: .atomic)
        } catch {
            scanError = "Couldn’t export snapshot: \(error.localizedDescription)"
        }
    }

    func formattedBytes(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(
            fromByteCount: Int64(min(bytes, UInt64(Int64.max))),
            countStyle: .file
        )
    }
}

private struct InspectorView: View {
    let root: SpaceLensNode
    @Binding var selectedNodeID: String?
    let onAddToCollector: (SpaceLensNode) -> Void

    private var children: [SpaceLensNode] {
        root.children.sorted { lhs, rhs in
            lhs.allocatedBytes == rhs.allocatedBytes ? lhs.name < rhs.name : lhs.allocatedBytes > rhs.allocatedBytes
        }
    }

    var body: some View {
        GlassSurface(cornerRadius: 24) {
            VStack(alignment: .leading, spacing: 14) {
                HStack(alignment: .top, spacing: 10) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(root.name).font(.title2.weight(.semibold))
                        Text(ByteCountFormatter.string(fromByteCount: Int64(root.allocatedBytes), countStyle: .file))
                            .font(.system(.title, design: .rounded).weight(.bold))
                        Text("Top-level distribution")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 0)
                    if let selectedNode, selectedNode.id != root.id, selectedNode.sourceURL != nil {
                        Button {
                            onAddToCollector(selectedNode)
                        } label: {
                            Image(systemName: "tray.and.arrow.down")
                        }
                        .buttonStyle(PhaseAwareGlassButtonStyle(prominent: false))
                        .help("Add selected item to Collector")
                    }
                }

                Divider()

                Text("Children")
                    .font(.headline)
                ScrollView {
                    LazyVStack(spacing: 6) {
                        ForEach(children) { child in
                            Button {
                                selectedNodeID = child.id
                            } label: {
                                HStack(spacing: 10) {
                                    Circle()
                                        .fill(rowColor(for: child))
                                        .frame(width: 8, height: 8)
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(child.name)
                                            .lineLimit(1)
                                        Text(ByteCountFormatter.string(fromByteCount: Int64(child.allocatedBytes), countStyle: .file))
                                            .font(.caption.monospacedDigit())
                                            .foregroundStyle(.secondary)
                                    }
                                    Spacer()
                                    Text(percentage(child.allocatedBytes, of: root.allocatedBytes))
                                        .font(.caption.monospacedDigit())
                                        .foregroundStyle(.secondary)
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                            }
                            .buttonStyle(.plain)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 8)
                            .background {
                                RoundedRectangle(cornerRadius: 10)
                                    .fill(selectedNodeID == child.id ? .white.opacity(0.10) : .white.opacity(0.035))
                            }
                            .contextMenu {
                                Button("Add to Collector") {
                                    onAddToCollector(child)
                                }
                                .disabled(child.sourceURL == nil || child.id == root.id)
                                Button("Reveal in Finder") {}
                                    .disabled(true)
                            }
                        }
                    }
                }
                .scrollIndicators(.hidden)
            }
        }
    }

    private func percentage(_ value: UInt64, of total: UInt64) -> String {
        guard total > 0 else { return "0%" }
        return "\(Int((Double(value) / Double(total) * 100).rounded()))%"
    }

    private func rowColor(for child: SpaceLensNode) -> Color {
        let index = children.firstIndex(of: child) ?? 0
        let hues = [0.54, 0.62, 0.70, 0.78, 0.87, 0.34, 0.25, 0.16, 0.46]
        return Color(hue: hues[index % hues.count], saturation: 0.64, brightness: 0.86)
    }

    private var selectedNode: SpaceLensNode? {
        root.node(withID: selectedNodeID)
    }
}

private struct CleanupReviewView: View {
    @Environment(\.dismiss) private var dismiss

    let entries: [CleanupEntry]
    let isProcessing: Bool
    let progress: TrashCleanupProgress?
    let onRemove: (CleanupEntry) -> Void
    let onMoveToTrash: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 5) {
                    Text("Review cleanup")
                        .font(.system(.title, design: .rounded).weight(.bold))
                    Text("These items will be moved to the macOS Trash. You can restore them from Finder before emptying it.")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer()
                Image(systemName: "trash")
                    .font(.title2)
                    .foregroundStyle(.orange)
            }

            if entries.isEmpty {
                ContentUnavailableView("Nothing staged", systemImage: "tray", description: Text("Return to the map and add a file or folder to the Collector."))
            } else {
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(entries) { entry in
                            HStack(spacing: 12) {
                                Image(systemName: "doc.fill")
                                    .foregroundStyle(.secondary)
                                VStack(alignment: .leading, spacing: 3) {
                                    Text(entry.name)
                                        .font(.body.weight(.medium))
                                        .lineLimit(1)
                                    Text(entry.url.path)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(1)
                                        .truncationMode(.middle)
                                }
                                Spacer(minLength: 8)
                                Text(formattedBytes(entry.allocatedBytes))
                                    .font(.caption.monospacedDigit())
                                    .foregroundStyle(.secondary)
                                Button {
                                    onRemove(entry)
                                } label: {
                                    Image(systemName: "xmark")
                                }
                                .buttonStyle(.borderless)
                                .disabled(isProcessing)
                                .help("Remove from Collector")
                            }
                            .padding(.horizontal, 12)
                            .padding(.vertical, 10)
                            .background(.white.opacity(0.045), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                        }
                    }
                }
                .scrollIndicators(.hidden)
            }

            if let progress, isProcessing {
                VStack(alignment: .leading, spacing: 7) {
                    ProgressView(value: progress.fraction)
                    Text("Moved \(progress.moved) of \(progress.total) items to Trash\(progress.currentName.map { " · \($0)" } ?? "")")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
            }

            Divider()

            HStack {
                VStack(alignment: .leading, spacing: 3) {
                    Text("\(entries.count) items")
                        .font(.headline)
                    Text(formattedBytes(entries.reduce(0) { $0 + $1.allocatedBytes }))
                        .font(.subheadline.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button("Done") {
                    dismiss()
                }
                .buttonStyle(PhaseAwareGlassButtonStyle(prominent: false))
                .disabled(isProcessing)
                Button("Move to Trash", role: .destructive) {
                    onMoveToTrash()
                }
                .buttonStyle(PhaseAwareGlassButtonStyle(prominent: true))
                .disabled(entries.isEmpty || isProcessing)
            }
        }
        .padding(24)
        .frame(minWidth: 680, minHeight: 480)
        .background(SpaceLensBackground())
        .interactiveDismissDisabled(isProcessing)
    }

    private func formattedBytes(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(
            fromByteCount: Int64(min(bytes, UInt64(Int64.max))),
            countStyle: .file
        )
    }
}

private struct PlaceholderView: View {
    let section: SidebarSection
    let back: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: section.icon)
                .font(.system(size: 42))
                .foregroundStyle(.tint)
            Text(section.title)
                .font(.title2.weight(.semibold))
            Text("This capability is part of a later Space Lens phase.")
                .foregroundStyle(.secondary)
            Button("Back to Home", action: back)
                .buttonStyle(PhaseAwareGlassButtonStyle(prominent: false))
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(SpaceLensBackground())
    }
}

struct SpaceLensBackground: View {
    var body: some View {
        Color(nsColor: .windowBackgroundColor)
            .overlay {
                Rectangle()
                    .fill(.regularMaterial)
                    .opacity(0.72)
            }
        .ignoresSafeArea()
    }
}
