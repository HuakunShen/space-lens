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
    public init() {}

    public func plan(for result: CloudScanResult) -> EvictionPlan {
        EvictionPlan(entries: result.items.filter(\.isEvictable).map(EvictionEntry.init(item:)))
    }

    public func execute(
        _ plan: EvictionPlan,
        dryRun: Bool,
        evictor: any CloudEvicting = FoundationCloudEvictor(),
        control: CloudOperationControl = CloudOperationControl(),
        progress: @escaping @Sendable (EvictionProgress) -> Void = { _ in }
    ) throws -> EvictionReport {
        if dryRun {
            return EvictionReport(wouldEvictCount: plan.entries.count)
        }

        var evictedCount = 0
        var failedCount = 0
        var freedBytes: Int64 = 0

        for (index, entry) in plan.entries.enumerated() {
            try control.waitIfNeeded()
            do {
                try evictor.evict(entry.url)
                evictedCount += 1
                freedBytes += entry.allocatedBytes
            } catch {
                failedCount += 1
            }

            progress(EvictionProgress(
                processedEntries: index + 1,
                totalEntries: plan.entries.count,
                evictedEntries: evictedCount,
                failedEntries: failedCount,
                freedBytes: freedBytes,
                currentURL: entry.url
            ))
        }

        return EvictionReport(
            evictedCount: evictedCount,
            failedCount: failedCount,
            freedBytes: freedBytes
        )
    }
}
