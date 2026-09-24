import Foundation

struct CleanupEntry: Identifiable, Hashable, Sendable {
    let id: String
    let name: String
    let url: URL
    let allocatedBytes: UInt64

    init?(node: SpaceLensNode) {
        guard let sourceURL = node.sourceURL else { return nil }
        id = node.id
        name = node.name
        url = sourceURL
        allocatedBytes = node.allocatedBytes
    }
}

struct CleanupCollector: Sendable {
    private(set) var entries: [CleanupEntry] = []

    var totalBytes: UInt64 {
        entries.reduce(0) { $0 + $1.allocatedBytes }
    }

    @discardableResult
    mutating func add(node: SpaceLensNode) -> Bool {
        guard let entry = CleanupEntry(node: node) else { return false }
        let candidatePath = Self.normalizedPath(entry.url)

        // Do not allow an item to be collected twice, or to collect a child
        // when one of its ancestors is already staged.
        if entries.contains(where: { Self.contains(Self.normalizedPath($0.url), candidatePath) }) {
            return false
        }

        // Selecting a parent supersedes any already-staged descendants.
        entries = entries.filter { !Self.contains(candidatePath, Self.normalizedPath($0.url)) }
        entries.append(entry)
        entries.sort { lhs, rhs in
            lhs.allocatedBytes == rhs.allocatedBytes
                ? lhs.name.localizedStandardCompare(rhs.name) == .orderedAscending
                : lhs.allocatedBytes > rhs.allocatedBytes
        }
        return true
    }

    mutating func remove(id: String) {
        entries.removeAll { $0.id == id }
    }

    mutating func remove(ids: Set<String>) {
        entries.removeAll { ids.contains($0.id) }
    }

    mutating func removeAll() {
        entries.removeAll()
    }

    private static func normalizedPath(_ url: URL) -> String {
        url.standardizedFileURL.path
    }

    private static func contains(_ parent: String, _ child: String) -> Bool {
        parent == child || child.hasPrefix(parent.hasSuffix("/") ? parent : parent + "/")
    }
}

struct TrashCleanupProgress: Equatable, Sendable {
    let processed: Int
    let total: Int
    let moved: Int
    let failed: Int
    let currentName: String?

    var fraction: Double {
        guard total > 0 else { return 0 }
        return min(max(Double(processed) / Double(total), 0), 1)
    }
}

struct TrashCleanupFailure: Identifiable, Sendable {
    let id: String
    let name: String
    let message: String
}

struct TrashCleanupReport: Sendable {
    let moved: [CleanupEntry]
    let failures: [TrashCleanupFailure]
}

struct TrashCleanupService: Sendable {
    func moveToTrash(
        _ entries: [CleanupEntry],
        progress: @escaping @Sendable (TrashCleanupProgress) -> Void = { _ in }
    ) -> TrashCleanupReport {
        var moved: [CleanupEntry] = []
        var failures: [TrashCleanupFailure] = []

        for (index, entry) in entries.enumerated() {
            do {
                var resultingURL: NSURL?
                try FileManager.default.trashItem(at: entry.url, resultingItemURL: &resultingURL)
                moved.append(entry)
            } catch {
                failures.append(TrashCleanupFailure(
                    id: entry.id,
                    name: entry.name,
                    message: error.localizedDescription
                ))
            }

            progress(TrashCleanupProgress(
                processed: index + 1,
                total: entries.count,
                moved: moved.count,
                failed: failures.count,
                currentName: entry.name
            ))
        }

        return TrashCleanupReport(moved: moved, failures: failures)
    }
}
