import Foundation

public enum CloudOperationError: Error, Equatable, LocalizedError, Sendable {
    case cancelled

    public var errorDescription: String? {
        switch self {
        case .cancelled:
            return "Operation cancelled."
        }
    }
}

public final class CloudOperationControl: @unchecked Sendable {
    private let condition = NSCondition()
    private var paused = false
    private var cancelled = false

    public init() {}

    public var isPaused: Bool {
        condition.lock()
        defer { condition.unlock() }
        return paused
    }

    public var isCancelled: Bool {
        condition.lock()
        defer { condition.unlock() }
        return cancelled
    }

    public func pause() {
        condition.lock()
        paused = true
        condition.unlock()
    }

    public func resume() {
        condition.lock()
        paused = false
        condition.broadcast()
        condition.unlock()
    }

    public func cancel() {
        condition.lock()
        cancelled = true
        paused = false
        condition.broadcast()
        condition.unlock()
    }

    public func waitIfNeeded() throws {
        condition.lock()
        while paused && !cancelled {
            condition.wait()
        }
        let wasCancelled = cancelled
        condition.unlock()

        if wasCancelled {
            throw CloudOperationError.cancelled
        }
    }
}

public struct CloudScanProgress: Equatable, Sendable {
    public let processedItems: Int
    public let discoveredItems: Int
    public let currentURL: URL?

    public init(processedItems: Int = 0, discoveredItems: Int = 0, currentURL: URL? = nil) {
        self.processedItems = processedItems
        self.discoveredItems = discoveredItems
        self.currentURL = currentURL
    }
}

public struct EvictionProgress: Equatable, Sendable {
    public let processedEntries: Int
    public let totalEntries: Int
    public let evictedEntries: Int
    public let failedEntries: Int
    public let freedBytes: Int64
    public let currentURL: URL?

    public init(
        processedEntries: Int = 0,
        totalEntries: Int = 0,
        evictedEntries: Int = 0,
        failedEntries: Int = 0,
        freedBytes: Int64 = 0,
        currentURL: URL? = nil
    ) {
        self.processedEntries = processedEntries
        self.totalEntries = totalEntries
        self.evictedEntries = evictedEntries
        self.failedEntries = failedEntries
        self.freedBytes = freedBytes
        self.currentURL = currentURL
    }

    public var fraction: Double {
        guard totalEntries > 0 else { return 0 }
        return min(max(Double(processedEntries) / Double(totalEntries), 0), 1)
    }
}
