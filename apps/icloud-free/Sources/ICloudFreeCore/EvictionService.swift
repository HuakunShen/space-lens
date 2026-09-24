import Foundation

public struct EvictionEntry: Codable, Sendable {
    public let url: URL
    public let name: String
    public let allocatedBytes: Int64

    public init(item: CloudItem) {
        url = item.url
        name = item.name
        allocatedBytes = item.allocatedSize
    }
}

public struct EvictionPlan: Codable, Sendable {
    public let entries: [EvictionEntry]

    public init(entries: [EvictionEntry]) {
        self.entries = entries
    }

    public var totalBytes: Int64 {
        entries.reduce(0) { $0 + $1.allocatedBytes }
    }
}

public struct EvictionReport: Codable, Sendable {
    public let wouldEvictCount: Int
    public let evictedCount: Int
    public let failedCount: Int
    public let freedBytes: Int64

    public init(wouldEvictCount: Int = 0, evictedCount: Int = 0, failedCount: Int = 0, freedBytes: Int64 = 0) {
        self.wouldEvictCount = wouldEvictCount
        self.evictedCount = evictedCount
        self.failedCount = failedCount
        self.freedBytes = freedBytes
    }
}

public protocol CloudEvicting: Sendable {
    func evict(_ url: URL) throws
}

public struct FoundationCloudEvictor: CloudEvicting {
    public init() {}

    public func evict(_ url: URL) throws {
        try FileManager.default.evictUbiquitousItem(at: url)
    }
}

public struct EvictionService: Sendable {
    public static let defaultConcurrency = 8
    public static let maximumConcurrency = 32

    public let maxConcurrency: Int

    public init(maxConcurrency: Int = EvictionService.defaultConcurrency) {
        self.maxConcurrency = min(max(maxConcurrency, 1), Self.maximumConcurrency)
    }

    public func plan(for result: CloudScanResult) -> EvictionPlan {
        EvictionPlan(entries: result.items.filter(\.isEvictable).map(EvictionEntry.init(item:)))
    }

    public func execute(
        _ plan: EvictionPlan,
        dryRun: Bool,
        evictor: any CloudEvicting = FoundationCloudEvictor(),
        control: CloudOperationControl = CloudOperationControl(),
        progress: @escaping @Sendable (EvictionProgress) -> Void = { _ in }
    ) async throws -> EvictionReport {
        if dryRun {
            return EvictionReport(wouldEvictCount: plan.entries.count)
        }

        let state = EvictionExecutionState(totalEntries: plan.entries.count)

        try await withThrowingTaskGroup(of: Void.self) { group in
            for _ in 0..<min(maxConcurrency, max(plan.entries.count, 1)) {
                group.addTask {
                    while let index = state.claimNextEntry() {
                        try control.waitIfNeeded()
                        let entry = plan.entries[index]
                        do {
                            try evictor.evict(entry.url)
                            progress(state.recordSuccess(for: entry))
                        } catch {
                            progress(state.recordFailure(for: entry))
                        }
                    }
                }
            }
            try await group.waitForAll()
        }

        return state.report()
    }
}

private final class EvictionExecutionState: @unchecked Sendable {
    private let lock = NSLock()
    private let totalEntries: Int
    private var nextIndex = 0
    private var processedEntries = 0
    private var evictedEntries = 0
    private var failedEntries = 0
    private var freedBytes: Int64 = 0
    private var activeEntries = 0

    init(totalEntries: Int) {
        self.totalEntries = totalEntries
    }

    func claimNextEntry() -> Int? {
        lock.lock()
        defer { lock.unlock() }
        guard nextIndex < totalEntries else { return nil }
        let index = nextIndex
        nextIndex += 1
        activeEntries += 1
        return index
    }

    func recordSuccess(for entry: EvictionEntry) -> EvictionProgress {
        lock.lock()
        defer { lock.unlock() }
        activeEntries = max(activeEntries - 1, 0)
        processedEntries += 1
        evictedEntries += 1
        freedBytes += entry.allocatedBytes
        return progress(currentURL: entry.url)
    }

    func recordFailure(for entry: EvictionEntry) -> EvictionProgress {
        lock.lock()
        defer { lock.unlock() }
        activeEntries = max(activeEntries - 1, 0)
        processedEntries += 1
        failedEntries += 1
        return progress(currentURL: entry.url)
    }

    func report() -> EvictionReport {
        lock.lock()
        defer { lock.unlock() }
        return EvictionReport(
            evictedCount: evictedEntries,
            failedCount: failedEntries,
            freedBytes: freedBytes
        )
    }

    private func progress(currentURL: URL) -> EvictionProgress {
        EvictionProgress(
            processedEntries: processedEntries,
            totalEntries: totalEntries,
            evictedEntries: evictedEntries,
            failedEntries: failedEntries,
            freedBytes: freedBytes,
            activeEntries: activeEntries,
            currentURL: currentURL
        )
    }
}
