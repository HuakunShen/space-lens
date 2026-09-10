import AppKit
import ICloudFreeAppSupport
import ICloudFreeCore
import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @ObservedObject var model: AppModel
    @State private var isDropTargeted = false
    @State private var filter: ItemFilter = .all
    @State private var sortOrder: [KeyPathComparator<CloudItem>] = [KeyPathComparator(\CloudItem.name)]

    var body: some View {
        ZStack {
            background

            VStack(spacing: 16) {
                header

                if model.isScanning || model.isExecuting {
                    operationBar
                }

                if let result = model.scanResult {
                    dashboard(result)
                } else {
                    emptyState
                }
            }
            .padding(.horizontal, 24)
            .padding(.vertical, 18)
            .padding(.top, 8)
        }
        .frame(minWidth: 980, minHeight: 680)
        .onDrop(of: [UTType.fileURL.identifier], isTargeted: $isDropTargeted) { providers in
            acceptDrop(providers)
        }
        .confirmationDialog(
            "Free local copies?",
            isPresented: $model.isShowingConfirmation,
            titleVisibility: .visible
        ) {
            Button("Free \(ByteCountFormatter.string(fromByteCount: model.evictionPlan.totalBytes, countStyle: .file))", role: .destructive) {
                Task { await model.executeEviction() }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("This keeps the files in iCloud Drive and removes only local copies. Files that are cloud-only are untouched.")
        }
        .alert(
            "Couldn’t inspect this folder",
            isPresented: Binding(
                get: { model.errorMessage != nil },
                set: { if !$0 { model.errorMessage = nil } }
            )
        ) {
            Button("OK") { model.errorMessage = nil }
        } message: {
            Text(model.errorMessage ?? "Unknown error")
        }
    }

    private var background: some View {
        ZStack {
            WindowMaterialBackground()
                .ignoresSafeArea()

            Color(nsColor: .windowBackgroundColor)
                .opacity(0.24)
                .ignoresSafeArea()
        }
    }

    private var header: some View {
        GlassSurface {
            HStack(spacing: 16) {
                SpaceLensLogoView()

                VStack(alignment: .leading, spacing: 3) {
                    Text("iCloud Free")
                        .font(.system(.title2, design: .rounded).weight(.bold))
                    Text("Inspect local copies before you free them")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }

                Spacer()

                if let root = model.selectedRoot {
                    Text(root.path)
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }

                Button {
                    chooseFolder()
                } label: {
                    Label("Choose Folder", systemImage: "folder.badge.plus")
                }
                .buttonStyle(.borderedProminent)
            }
        }
    }

    @ViewBuilder
    private func dashboard(_ result: CloudScanResult) -> some View {
        let cards = HStack(spacing: 12) {
            metric("Reclaimable", value: bytes(result.reclaimableBytes), icon: "arrow.down.circle", tint: .blue)
            metric("Local files", value: "\(result.localFiles)", icon: "internaldrive", tint: .green)
            metric("Cloud only", value: "\(result.cloudOnlyFiles)", icon: "icloud", tint: .indigo)
            metric("Logical size", value: bytes(result.logicalBytes), icon: "doc.text.magnifyingglass", tint: .purple)
        }

        VStack(spacing: 14) {
            if #available(macOS 26.0, *) {
                GlassEffectContainer(spacing: 12) { cards }
            } else {
                cards
            }

            GlassSurface {
                VStack(spacing: 14) {
                    HStack {
                        Picker("Show", selection: $filter) {
                            ForEach(ItemFilter.allCases) { filter in
                                Text(filter.title).tag(filter)
                            }
                        }
                        .pickerStyle(.segmented)
                        .frame(width: 300)

                        Spacer()

                    }

                    Table(filteredItems(result.items), sortOrder: $sortOrder) {
                        TableColumn("File", value: \CloudItem.name) { item in
                            HStack(spacing: 8) {
                                Image(systemName: item.isDirectory ? "folder" : "doc")
                                    .foregroundStyle(.secondary)
                                Text(item.name)
                                    .lineLimit(1)
                            }
                        }
                        TableColumn("Status", value: \CloudItem.status) { item in
                            StatusBadge(status: item.status)
                        }
                        TableColumn("Local", value: \CloudItem.allocatedSize) { item in
                            Text(bytes(item.allocatedSize))
                                .monospacedDigit()
                                .foregroundStyle(item.isEvictable ? .primary : .secondary)
                        }
                        TableColumn("Logical", value: \CloudItem.logicalSize) { item in
                            Text(bytes(item.logicalSize))
                                .monospacedDigit()
                                .foregroundStyle(.secondary)
                        }
                    }
                    .frame(minHeight: 270)
                }
            }

            GlassSurface {
                HStack {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(model.isScanning ? "Scanning local copies…" : model.isExecuting ? "Freeing local copies…" : model.canEvict ? "Ready to free local copies" : "Nothing to free yet")
                            .font(.headline)
                        Text(model.isScanning ? "\(model.scanProgress?.processedItems ?? 0) items inspected" : model.isExecuting ? evictionProgressSummary : model.canEvict ? "\(model.evictionPlan.entries.count) files · \(bytes(model.evictionPlan.totalBytes))" : "Choose or drop an iCloud Drive folder to begin")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button {
                        model.requestEvictionConfirmation()
                    } label: {
                        Label("Free Local Copies", systemImage: "arrow.down.circle.fill")
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(!model.canEvict)
                }
            }
        }
    }

    private var operationBar: some View {
        GlassSurface {
            HStack(spacing: 14) {
                Image(systemName: model.isExecuting ? "arrow.down.circle.fill" : "magnifyingglass.circle.fill")
                    .font(.title3)
                    .foregroundStyle(model.isOperationPaused ? .orange : .blue)

                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 8) {
                        Text(model.isOperationPaused ? "Paused" : model.isExecuting ? "Freeing local copies" : "Scanning iCloud Drive")
                            .font(.headline)
                        if let operationStatus = model.operationStatus {
                            Text(operationStatus)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                    }

                    if let progress = model.evictionProgress {
                        ProgressView(value: progress.fraction)
                            .progressViewStyle(.linear)
                        Text("\(progress.processedEntries) of \(progress.totalEntries) files · \(bytes(progress.freedBytes)) freed")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.secondary)
                    } else {
                        ProgressView()
                            .progressViewStyle(.linear)
                        Text("\(model.scanProgress?.processedItems ?? 0) items inspected")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.secondary)
                    }
                }

                Spacer()

                Button(model.isOperationPaused ? "Resume" : "Pause") {
                    model.togglePause()
                }
                .buttonStyle(.bordered)

                Button("Stop", role: .cancel) {
                    model.cancelOperation()
                }
                .buttonStyle(.bordered)
            }
        }
    }

    private var evictionProgressSummary: String {
        guard let progress = model.evictionProgress else { return "Preparing…" }
        return "\(progress.processedEntries) of \(progress.totalEntries) files · \(bytes(progress.freedBytes)) freed"
    }

    private var emptyState: some View {
        GlassSurface {
            VStack(spacing: 14) {
                Image(systemName: isDropTargeted ? "arrow.down.doc.fill" : "folder.badge.questionmark")
                    .font(.system(size: 46, weight: .medium))
                    .foregroundStyle(isDropTargeted ? .blue : .secondary)
                Text(isDropTargeted ? "Drop the folder here" : "Drop an iCloud Drive folder here")
                    .font(.title3.weight(.semibold))
                Text("We’ll show what is actually stored locally and what can be freed without deleting the iCloud files.")
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: 500)
                Button("Choose Folder…") { chooseFolder() }
                    .buttonStyle(.borderedProminent)
            }
            .frame(maxWidth: .infinity, minHeight: 330)
        }
    }

    private func metric(_ title: String, value: String, icon: String, tint: Color) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Image(systemName: icon)
                .font(.title3)
                .foregroundStyle(tint)
            Text(value)
                .font(.system(.title2, design: .rounded).weight(.bold))
                .lineLimit(1)
                .minimumScaleFactor(0.7)
            Text(title)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(16)
    }

    private func filteredItems(_ items: [CloudItem]) -> [CloudItem] {
        switch filter {
        case .all: return items
        case .local: return items.filter { $0.status == .local }
        case .cloud: return items.filter { $0.status == .cloudOnly }
        case .skipped: return items.filter { !$0.isEvictable && $0.status != .cloudOnly }
        }
    }

    private func chooseFolder() {
        guard let url = FolderPicker.chooseFolder() else { return }
        Task { await model.scan(root: url) }
    }

    private func acceptDrop(_ providers: [NSItemProvider]) -> Bool {
        guard let provider = providers.first else { return false }
        provider.loadObject(ofClass: NSURL.self) { object, _ in
            guard let object = object as? NSURL else { return }
            let url = object as URL
            Task { @MainActor in await model.scan(root: url) }
        }
        return true
    }

    private func bytes(_ value: Int64) -> String {
        ByteCountFormatter.string(fromByteCount: value, countStyle: .file)
    }
}

private enum ItemFilter: String, CaseIterable, Identifiable {
    case all
    case local
    case cloud
    case skipped

    var id: String { rawValue }

    var title: String {
        switch self {
        case .all: return "All"
        case .local: return "Local"
        case .cloud: return "Cloud only"
        case .skipped: return "Skipped"
        }
    }
}

private struct StatusBadge: View {
    let status: CloudItemStatus

    var body: some View {
        Label(status.title, systemImage: status.icon)
            .font(.caption)
            .foregroundStyle(status.tint)
    }
}

private struct SpaceLensLogoView: View {
    private static let image: NSImage? = {
        let urls = [
            Bundle.module.url(forResource: "SpaceLensLogo", withExtension: "png"),
            Bundle.main.url(forResource: "SpaceLensLogo", withExtension: "png"),
        ].compactMap { $0 }

        return urls.lazy.compactMap(NSImage.init(contentsOf:)).first
    }()

    var body: some View {
        Group {
            if let image = Self.image {
                Image(nsImage: image)
                    .resizable()
                    .scaledToFit()
            } else {
                Image(systemName: "scope")
                    .font(.system(size: 24, weight: .medium))
                    .foregroundStyle(.blue)
            }
        }
        .frame(width: 38, height: 38)
        .background(.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 11, style: .continuous))
        .clipShape(RoundedRectangle(cornerRadius: 11, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 11, style: .continuous)
                .stroke(.white.opacity(0.24), lineWidth: 0.6)
        }
        .accessibilityLabel("Space Lens logo")
    }
}

private extension CloudItemStatus {
    var title: String {
        switch self {
        case .local: return "Local"
        case .cloudOnly: return "Cloud only"
        case .downloading: return "Downloading"
        case .uploading: return "Uploading"
        case .excluded: return "Excluded"
        case .pinned: return "Kept downloaded"
        case .notICloud: return "Not iCloud"
        case .symlink: return "Symlink"
        case .directory: return "Folder"
        case .unknown: return "Unknown"
        }
    }

    var icon: String {
        switch self {
        case .local: return "internaldrive"
        case .cloudOnly: return "icloud"
        case .downloading: return "arrow.down.circle"
        case .uploading: return "arrow.up.circle"
        case .excluded: return "nosign"
        case .pinned: return "pin.fill"
        case .notICloud: return "questionmark.circle"
        case .symlink: return "link"
        case .directory: return "folder"
        case .unknown: return "questionmark.circle"
        }
    }

    var tint: Color {
        switch self {
        case .local: return .green
        case .cloudOnly: return .indigo
        case .downloading, .uploading: return .orange
        case .pinned: return .cyan
        default: return .secondary
        }
    }
}
