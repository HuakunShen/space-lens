import AppKit
import Foundation
import SpaceLensFFIBridge
import SwiftUI

private enum RustICloudError: LocalizedError {
    case unavailable
    case invalidPlan
    case executionFailed

    var errorDescription: String? {
        switch self {
        case .unavailable:
            "Couldn’t create a scan for this path. Make sure it is an iCloud Drive item."
        case .invalidPlan:
            "The scan returned an invalid plan."
        case .executionFailed:
            "Couldn’t execute the scan plan."
        }
    }
}

private struct RustICloudCandidate: Decodable, Sendable {
    let path: String
    let logicalBytes: String
    let allocatedBytes: String
}

private struct RustICloudSummary: Decodable, Sendable {
    let path: String
    let logicalBytes: String
    let allocatedBytes: String?
    let state: String
    let reason: String?
}

private struct RustICloudSkipped: Decodable, Sendable {
    let path: String
    let reason: String
}

private enum RustICloudRowStatus: String, CaseIterable, Comparable, Identifiable, Sendable {
    case local
    case cloudOnly
    case skipped

    var id: String { rawValue }

    static func < (lhs: RustICloudRowStatus, rhs: RustICloudRowStatus) -> Bool {
        lhs.rawValue < rhs.rawValue
    }

    var title: String {
        switch self {
        case .local: "Local · reclaimable"
        case .cloudOnly: "Cloud only"
        case .skipped: "Skipped"
        }
    }

    var icon: String {
        switch self {
        case .local: "internaldrive"
        case .cloudOnly: "icloud"
        case .skipped: "nosign"
        }
    }

    var tint: Color {
        switch self {
        case .local: .green
        case .cloudOnly: .indigo
        case .skipped: .secondary
        }
    }
}

private struct RustICloudRow: Identifiable, Hashable, Sendable {
    let id: String
    let name: String
    let path: String
    let logicalBytes: UInt64
    let allocatedBytes: UInt64
    let status: RustICloudRowStatus
    let reason: String?

    init(path: String, logicalBytes: UInt64, allocatedBytes: UInt64, status: RustICloudRowStatus, reason: String? = nil) {
        id = path
        self.path = path
        name = URL(fileURLWithPath: path).lastPathComponent
        self.logicalBytes = logicalBytes
        self.allocatedBytes = allocatedBytes
        self.status = status
        self.reason = reason
    }
}

private struct RustICloudPlan: Decodable, Sendable {
    let root: String
    let candidates: [RustICloudCandidate]
    let cloudOnly: [RustICloudSummary]
    let skipped: [RustICloudSkipped]
    let coverageComplete: Bool
    let visitedEntries: Int
    let notesTotal: Int

    var rows: [RustICloudRow] {
        let candidateRows = candidates.map { candidate in
            RustICloudRow(
                path: candidate.path,
                logicalBytes: bytes(candidate.logicalBytes),
                allocatedBytes: bytes(candidate.allocatedBytes),
                status: .local
            )
        }
        let cloudRows = cloudOnly.map { item in
            RustICloudRow(
                path: item.path,
                logicalBytes: bytes(item.logicalBytes),
                allocatedBytes: bytes(item.allocatedBytes),
                status: .cloudOnly,
                reason: item.reason
            )
        }
        let skippedRows = skipped.map { item in
            RustICloudRow(
                path: item.path,
                logicalBytes: 0,
                allocatedBytes: 0,
                status: .skipped,
                reason: item.reason
            )
        }
        return (candidateRows + cloudRows + skippedRows).sorted {
            $0.allocatedBytes == $1.allocatedBytes
                ? $0.name.localizedStandardCompare($1.name) == .orderedAscending
                : $0.allocatedBytes > $1.allocatedBytes
        }
    }

    var reclaimableBytes: UInt64 {
        candidates.reduce(0) { $0 + bytes($1.allocatedBytes) }
    }

    var logicalBytes: UInt64 {
        candidates.reduce(0) { $0 + bytes($1.logicalBytes) }
            + cloudOnly.reduce(0) { $0 + bytes($1.logicalBytes) }
    }

    private func bytes(_ value: String?) -> UInt64 {
        UInt64(value ?? "0") ?? 0
    }
}

private struct RustICloudProgressSnapshot: Sendable {
    let processed: UInt64
    let total: UInt64
    let evicted: UInt64
    let failed: UInt64
    let freedBytes: UInt64
    let active: UInt64
    let running: Bool
    let paused: Bool
    let cancelled: Bool

    var fraction: Double {
        guard total > 0 else { return 0 }
        return min(max(Double(processed) / Double(total), 0), 1)
    }
}

private struct RustICloudExecutionReport: Decodable, Sendable {
    let results: [RustICloudExecutionResult]
    let cancelled: Bool
}

private struct RustICloudExecutionResult: Decodable, Sendable {
    let path: String
    let status: String
}

private final class RustICloudSession: @unchecked Sendable {
    private let handle: OpaquePointer

    init(path: String) throws {
        guard let handle = path.withCString({ sl_icloud_plan_create($0) }) else {
            throw RustICloudError.unavailable
        }
        self.handle = handle
    }

    deinit {
        sl_icloud_plan_free(handle)
    }

    func readPlan() throws -> RustICloudPlan {
        let data = try readBuffer(sl_icloud_plan_handle_json(handle))
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(RustICloudPlan.self, from: data)
    }

    func execute(maxConcurrency: UInt32) throws -> RustICloudExecutionReport {
        let data = try readBuffer(sl_icloud_plan_execute(handle, maxConcurrency))
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(RustICloudExecutionReport.self, from: data)
    }

    func progress() -> RustICloudProgressSnapshot {
        let raw = sl_icloud_progress(handle)
        return RustICloudProgressSnapshot(
            processed: raw.processed,
            total: raw.total,
            evicted: raw.evicted,
            failed: raw.failed,
            freedBytes: raw.freed_bytes,
            active: raw.active,
            running: raw.running != 0,
            paused: raw.paused != 0,
            cancelled: raw.cancelled != 0
        )
    }

    func pause() { sl_icloud_pause(handle) }
    func resume() { sl_icloud_resume(handle) }
    func cancel() { sl_icloud_cancel(handle) }

    private func readBuffer(_ buffer: SlOwnedBuffer) throws -> Data {
        guard let pointer = buffer.ptr, buffer.len > 0 else {
            throw RustICloudError.executionFailed
        }
        defer { sl_free_buffer(buffer) }
        return Data(bytes: pointer, count: Int(buffer.len))
    }
}

@MainActor
private final class RustICloudModel: ObservableObject {
    @Published var selectedRoot: URL?
    @Published var plan: RustICloudPlan?
    @Published var isScanning = false
    @Published var isExecuting = false
    @Published var isPaused = false
    @Published var progress: RustICloudProgressSnapshot?
    @Published var status: String?
    @Published var errorMessage: String?

    private var session: RustICloudSession?
    private var executionTask: Task<Void, Never>?

    var canExecute: Bool {
        guard let plan else { return false }
        return !plan.candidates.isEmpty && !isScanning && !isExecuting
    }

    func scan(root: URL) async {
        executionTask?.cancel()
        isScanning = true
        isExecuting = false
        isPaused = false
        errorMessage = nil
        status = "Scanning iCloud…"
        plan = nil
        progress = nil

        do {
            let session = try await Task.detached(priority: .userInitiated) {
                try RustICloudSession(path: root.standardizedFileURL.path)
            }.value
            let plan = try await Task.detached(priority: .userInitiated) {
                try session.readPlan()
            }.value
            self.session = session
            selectedRoot = root.standardizedFileURL
            self.plan = plan
            status = plan.coverageComplete
                ? "Scan complete"
                : "Scan incomplete; discovered files can still be freed"
        } catch {
            errorMessage = error.localizedDescription
            status = "Scan failed"
        }
        isScanning = false
    }

    func beginExecution() {
        guard canExecute, let session, let selectedRoot else { return }
        isExecuting = true
        isPaused = false
        errorMessage = nil
        status = "Freeing local copies…"
        progress = session.progress()

        executionTask = Task { @MainActor [weak self] in
            let execution = Task.detached(priority: .userInitiated) {
                try session.execute(maxConcurrency: 8)
            }

            while true {
                try? await Task.sleep(nanoseconds: 100_000_000)
                let snapshot = session.progress()
                self?.progress = snapshot
                if !snapshot.running { break }
            }

            do {
                let report = try await execution.value
                guard let self else { return }
                isExecuting = false
                isPaused = false
                if report.cancelled {
                    status = "Freeing cancelled"
                    return
                }
                status = "Freeing finished"
                await scan(root: selectedRoot)
            } catch {
                self?.isExecuting = false
                self?.isPaused = false
                self?.errorMessage = error.localizedDescription
                self?.status = "Freeing failed"
            }
        }
    }

    func togglePause() {
        guard isExecuting else { return }
        if isPaused {
            session?.resume()
            isPaused = false
            status = "Resuming…"
        } else {
            session?.pause()
            isPaused = true
            status = "Paused"
        }
    }

    func cancel() {
        session?.cancel()
        status = "Cancelling…"
    }

    func rows(for filter: RustICloudFilter) -> [RustICloudRow] {
        guard let plan else { return [] }
        switch filter {
        case .all: return plan.rows
        case .local: return plan.rows.filter { $0.status == .local }
        case .cloudOnly: return plan.rows.filter { $0.status == .cloudOnly }
        case .skipped: return plan.rows.filter { $0.status == .skipped }
        }
    }
}

struct ICloudLocalCopiesView: View {
    @StateObject private var model = RustICloudModel()
    @State private var filter: RustICloudFilter = .all
    @State private var sortOrder: [KeyPathComparator<RustICloudRow>] = [
        KeyPathComparator(\RustICloudRow.allocatedBytes, order: .reverse)
    ]
    @State private var showingConfirmation = false

    var body: some View {
        ZStack {
            SpaceLensBackground()
            ScrollView(.vertical) {
                VStack(spacing: 14) {
                    header
                    if model.isScanning || model.isExecuting { operationBar }
                    if let plan = model.plan { dashboard(plan) } else { emptyState }
                }
                .frame(maxWidth: .infinity, alignment: .topLeading)
                .padding(20)
            }
            .scrollIndicators(.automatic)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .confirmationDialog("Free local copies?", isPresented: $showingConfirmation, titleVisibility: .visible) {
            Button("Free \(formattedBytes(model.plan?.reclaimableBytes ?? 0))", role: .destructive) {
                model.beginExecution()
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("The app will revalidate every discovered candidate before freeing it. Cloud-only, skipped, and unvisited items are untouched; no download is requested.")
        }
        .alert("Couldn’t complete iCloud operation", isPresented: Binding(
            get: { model.errorMessage != nil },
            set: { if !$0 { model.errorMessage = nil } }
        )) {
            Button("OK") { model.errorMessage = nil }
        } message: {
            Text(model.errorMessage ?? "Unknown iCloud operation error")
        }
    }

    private var header: some View {
        GlassSurface(cornerRadius: 20) {
            ViewThatFits(in: .horizontal) {
                wideHeader
                compactHeader
            }
        }
    }

    private var wideHeader: some View {
        HStack(spacing: 14) {
            headerIcon
            headerTitle
                .layoutPriority(1)
            Spacer(minLength: 8)
            if let root = model.selectedRoot {
                Text(root.path)
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .frame(minWidth: 120, maxWidth: 260, alignment: .trailing)
            }
            chooseFolderButton
        }
    }

    private var compactHeader: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 12) {
                headerIcon
                headerTitle
                    .layoutPriority(1)
                Spacer(minLength: 0)
            }

            if let root = model.selectedRoot {
                Text(root.path)
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                    .truncationMode(.middle)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }

            chooseFolderButton
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var headerIcon: some View {
        Image(systemName: "icloud.and.arrow.down")
            .font(.system(size: 28, weight: .medium))
            .foregroundStyle(.indigo)
            .frame(width: 42, height: 42)
            .background(.indigo.opacity(0.12), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
    }

    private var headerTitle: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text("iCloud Local Copies")
                .font(.system(.title2, design: .rounded).weight(.bold))
                .lineLimit(1)
                .minimumScaleFactor(0.82)
            Text("Metadata-only scan · no cloud-only downloads")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var chooseFolderButton: some View {
        Button { chooseFolder() } label: {
            Label("Choose Folder", systemImage: "folder.badge.plus")
        }
        .buttonStyle(PhaseAwareGlassButtonStyle(prominent: true))
        .disabled(model.isScanning || model.isExecuting)
    }

    @ViewBuilder
    private func dashboard(_ plan: RustICloudPlan) -> some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 12) {
                metrics(plan)
            }
            LazyVGrid(
                columns: [GridItem(.adaptive(minimum: 150), spacing: 12)],
                spacing: 12
            ) {
                metrics(plan)
            }
        }

        GlassSurface(cornerRadius: 20) {
            VStack(spacing: 12) {
                ViewThatFits(in: .horizontal) {
                    HStack {
                        filterPicker
                        Spacer(minLength: 8)
                        coverageStatus(plan)
                    }
                    VStack(alignment: .leading, spacing: 10) {
                        filterPicker
                            .frame(maxWidth: .infinity, alignment: .leading)
                        coverageStatus(plan)
                    }
                }

                ViewThatFits(in: .horizontal) {
                    tableView
                        .frame(minWidth: 680, minHeight: 290)
                    ScrollView(.horizontal) {
                        tableView
                            .frame(minWidth: 680, minHeight: 290)
                    }
                    .scrollIndicators(.automatic)
                }
                .frame(minHeight: 290)
            }
        }

        GlassSurface(cornerRadius: 18) {
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 12) {
                    actionSummary(plan)
                    Spacer(minLength: 12)
                    freeButton
                }
                VStack(alignment: .leading, spacing: 12) {
                    actionSummary(plan)
                    HStack {
                        Spacer(minLength: 0)
                        freeButton
                    }
                }
            }
        }
    }

    private func metrics(_ plan: RustICloudPlan) -> some View {
        Group {
            metric("Reclaimable", value: formattedBytes(plan.reclaimableBytes), icon: "arrow.down.circle", tint: .blue)
            metric("Local files", value: "\(plan.candidates.count)", icon: "internaldrive", tint: .green)
            metric("Cloud only", value: "\(plan.cloudOnly.count)", icon: "icloud", tint: .indigo)
            metric("Visited", value: "\(plan.visitedEntries)", icon: "checklist", tint: .purple)
        }
    }

    private var filterPicker: some View {
        Picker("Show", selection: $filter) {
            ForEach(RustICloudFilter.allCases) { filter in
                Text(filter.title).tag(filter)
            }
        }
        .pickerStyle(.segmented)
        .frame(maxWidth: 330)
    }

    private func coverageStatus(_ plan: RustICloudPlan) -> some View {
        Text(plan.coverageComplete
            ? "Scan complete"
            : "Incomplete scan · discovered files can be freed")
            .font(.caption)
            .foregroundStyle(plan.coverageComplete ? Color.secondary : Color.orange)
            .lineLimit(2)
    }

    private var tableView: some View {
        Table(model.rows(for: filter), sortOrder: $sortOrder) {
            TableColumn("File", value: \RustICloudRow.name) { row in
                VStack(alignment: .leading, spacing: 2) {
                    Text(row.name).lineLimit(1)
                    if let reason = row.reason {
                        Text(reason).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                }
            }
            TableColumn("Status", value: \RustICloudRow.status) { row in
                Label(row.status.title, systemImage: row.status.icon)
                    .font(.caption)
                    .foregroundStyle(row.status.tint)
            }
            TableColumn("Local", value: \RustICloudRow.allocatedBytes) { row in
                Text(formattedBytes(row.allocatedBytes)).monospacedDigit()
            }
            TableColumn("Logical", value: \RustICloudRow.logicalBytes) { row in
                Text(formattedBytes(row.logicalBytes)).monospacedDigit().foregroundStyle(.secondary)
            }
        }
    }

    private func actionSummary(_ plan: RustICloudPlan) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(model.isExecuting
                ? "Freeing local copies…"
                : model.canExecute
                    ? plan.coverageComplete ? "Ready to free local copies" : "Ready to free discovered copies"
                    : "Nothing safe to free yet")
                .font(.headline)
            Text(model.isExecuting
                ? progressSummary
                : model.canExecute
                    ? "\(plan.candidates.count) files · \(formattedBytes(plan.reclaimableBytes))"
                    : model.status ?? "Choose an iCloud Drive folder to begin")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var freeButton: some View {
        Button {
            showingConfirmation = true
        } label: {
            Label(
                model.plan?.coverageComplete == false ? "Free Discovered Copies" : "Free Local Copies",
                systemImage: "arrow.down.circle.fill"
            )
        }
        .buttonStyle(PhaseAwareGlassButtonStyle(prominent: true))
        .disabled(!model.canExecute)
    }

    private var emptyState: some View {
        GlassSurface(cornerRadius: 22) {
            VStack(spacing: 14) {
                Image(systemName: "icloud.and.arrow.down")
                    .font(.system(size: 48, weight: .medium))
                    .foregroundStyle(.indigo)
                Text("Inspect iCloud local copies")
                    .font(.title2.weight(.semibold))
                Text("Reads native iCloud metadata, skips cloud-only items, and never requests a download during scanning.")
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: 540)
                Button("Choose iCloud Drive Folder…") { chooseFolder() }
                    .buttonStyle(PhaseAwareGlassButtonStyle(prominent: true))
                    .disabled(model.isScanning || model.isExecuting)
            }
            .frame(maxWidth: .infinity, minHeight: 340)
        }
    }

    private var operationBar: some View {
        GlassSurface(cornerRadius: 18) {
            HStack(spacing: 14) {
                Image(systemName: model.isExecuting ? "arrow.down.circle.fill" : "magnifyingglass.circle.fill")
                    .font(.title3)
                    .foregroundStyle(model.isPaused ? .orange : .blue)
                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 8) {
                        Text(model.isPaused ? "Paused" : model.isExecuting ? "Freeing local copies" : "Scanning")
                            .font(.headline)
                        if let status = model.status {
                            Text(status).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                    }
                    if let progress = model.progress, model.isExecuting {
                        ProgressView(value: progress.fraction)
                        Text(progressSummary).font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                    } else {
                        ProgressView()
                        Text("Scanning without downloading cloud-only files")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
                Spacer()
                if model.isExecuting {
                    Button(model.isPaused ? "Resume" : "Pause") { model.togglePause() }
                        .buttonStyle(PhaseAwareGlassButtonStyle(prominent: false))
                    Button("Cancel", role: .cancel) { model.cancel() }
                        .buttonStyle(PhaseAwareGlassButtonStyle(prominent: false))
                }
            }
        }
    }

    private var progressSummary: String {
        guard let progress = model.progress else { return "Preparing…" }
        return "\(progress.processed) of \(progress.total) files · \(progress.active) active · \(formattedBytes(progress.freedBytes)) freed"
    }

    private func metric(_ title: String, value: String, icon: String, tint: Color) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Image(systemName: icon).font(.title3).foregroundStyle(tint)
            Text(value).font(.system(.title2, design: .rounded).weight(.bold)).lineLimit(1).minimumScaleFactor(0.7)
            Text(title).font(.caption).foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .frame(minWidth: 128, maxWidth: .infinity, alignment: .leading)
        .padding(14)
    }

    private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = false
        panel.allowsMultipleSelection = false
        panel.prompt = "Scan Folder"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task { await model.scan(root: url) }
    }

    private func formattedBytes(_ value: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(min(value, UInt64(Int64.max))), countStyle: .file)
    }
}

private enum RustICloudFilter: String, CaseIterable, Identifiable {
    case all
    case local
    case cloudOnly
    case skipped

    var id: String { rawValue }

    var title: String {
        switch self {
        case .all: "All"
        case .local: "Local"
        case .cloudOnly: "Cloud only"
        case .skipped: "Skipped"
        }
    }
}
