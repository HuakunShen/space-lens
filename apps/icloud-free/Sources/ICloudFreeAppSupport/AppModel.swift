import Combine
import Foundation
import ICloudFreeCore

@MainActor
public final class AppModel: ObservableObject {
    @Published public private(set) var selectedRoot: URL?
    @Published public private(set) var scanResult: CloudScanResult?
    @Published public private(set) var evictionReport: EvictionReport?
    @Published public private(set) var isScanning = false
    @Published public private(set) var isExecuting = false
    @Published public private(set) var isOperationPaused = false
    @Published public private(set) var scanProgress: CloudScanProgress?
    @Published public private(set) var evictionProgress: EvictionProgress?
    @Published public private(set) var operationStatus: String?
    @Published public var isShowingConfirmation = false
    @Published public var errorMessage: String?

    private let scanner: CloudScanner
    private let evictionService: EvictionService
    private let evictor: any CloudEvicting
    private var scanControl: CloudOperationControl?
    private var evictionControl: CloudOperationControl?

    public init(
        scanner: CloudScanner = CloudScanner(),
        evictionService: EvictionService = EvictionService(),
        evictor: any CloudEvicting = FoundationCloudEvictor()
    ) {
        self.scanner = scanner
        self.evictionService = evictionService
        self.evictor = evictor
    }

    public var evictionPlan: EvictionPlan {
        guard let scanResult else { return EvictionPlan(entries: []) }
        return evictionService.plan(for: scanResult)
    }

    public var canEvict: Bool {
        !evictionPlan.entries.isEmpty && !isScanning && !isExecuting
    }

    public func scan(root: URL) async {
        selectedRoot = root
        scanResult = nil
        evictionReport = nil
        errorMessage = nil
        operationStatus = "Preparing scan…"
        scanProgress = CloudScanProgress(currentURL: root)
        evictionProgress = nil
        isShowingConfirmation = false
        isScanning = true
        isOperationPaused = false

        let control = CloudOperationControl()
        scanControl = control
        let progressStream = AsyncStream.makeStream(of: CloudScanProgress.self)
        let progressConsumer = Task { @MainActor in
            for await progress in progressStream.stream {
                scanProgress = progress
                operationStatus = progress.currentURL.map { "Scanning \($0.lastPathComponent)…" } ?? "Scanning…"
            }
        }

        do {
            let scanner = scanner
            let result = try await Task.detached(priority: .userInitiated) {
                defer { progressStream.continuation.finish() }
                return try scanner.scan(
                    root: root,
                    recursive: true,
                    control: control,
                    progress: { progressStream.continuation.yield($0) }
                )
            }.value
            scanResult = result
            operationStatus = "Scan complete"
        } catch CloudOperationError.cancelled {
            operationStatus = "Scan cancelled"
        } catch {
            errorMessage = error.localizedDescription
            operationStatus = "Scan failed"
        }

        await progressConsumer.value
        scanControl = nil
        isScanning = false
        isOperationPaused = false
    }

    public func requestEvictionConfirmation() {
        guard canEvict else { return }
        isShowingConfirmation = true
    }

    public func cancelEviction() {
        isShowingConfirmation = false
    }

    public func executeEviction() async {
        guard canEvict else { return }
        isShowingConfirmation = false
        isExecuting = true
        errorMessage = nil
        operationStatus = "Preparing eviction…"
        evictionProgress = EvictionProgress(totalEntries: evictionPlan.entries.count)
        isOperationPaused = false

        let service = evictionService
        let plan = evictionPlan
        let evictor = evictor
        let control = CloudOperationControl()
        evictionControl = control
        let progressStream = AsyncStream.makeStream(of: EvictionProgress.self)
        let progressConsumer = Task { @MainActor in
            for await progress in progressStream.stream {
                evictionProgress = progress
                operationStatus = progress.currentURL.map { "Freeing \($0.lastPathComponent)…" } ?? "Freeing local copies…"
            }
        }

        do {
            evictionReport = try await Task.detached(priority: .userInitiated) {
                defer { progressStream.continuation.finish() }
                return try service.execute(
                    plan,
                    dryRun: false,
                    evictor: evictor,
                    control: control,
                    progress: { progressStream.continuation.yield($0) }
                )
            }.value
            operationStatus = "Finished freeing local copies"
        } catch CloudOperationError.cancelled {
            operationStatus = "Eviction cancelled"
        } catch {
            errorMessage = error.localizedDescription
            operationStatus = "Eviction failed"
        }

        await progressConsumer.value
        evictionControl = nil
        isExecuting = false
        isOperationPaused = false
    }

    public func togglePause() {
        guard isScanning || isExecuting else { return }

        if isOperationPaused {
            scanControl?.resume()
            evictionControl?.resume()
            isOperationPaused = false
            operationStatus = isScanning ? "Resuming scan…" : "Resuming eviction…"
        } else {
            scanControl?.pause()
            evictionControl?.pause()
            isOperationPaused = true
            operationStatus = isScanning ? "Scan paused" : "Eviction paused"
        }
    }

    public func cancelOperation() {
        guard isScanning || isExecuting else { return }
        scanControl?.cancel()
        evictionControl?.cancel()
    }
}
