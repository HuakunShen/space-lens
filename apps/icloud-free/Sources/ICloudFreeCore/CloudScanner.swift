import Darwin
import Foundation

public protocol CloudResourceReading: Sendable {
    func snapshot(for url: URL) throws -> CloudItemSnapshot
    func children(of url: URL) throws -> [URL]
}

public struct CloudScanner: Sendable {
    private let reader: any CloudResourceReading

    public init() {
        reader = FoundationCloudResourceReader()
    }

    public init(reader: any CloudResourceReading) {
        self.reader = reader
    }

    public func scan(
        root: URL,
        recursive: Bool = true,
        control: CloudOperationControl = CloudOperationControl(),
        progress: @escaping @Sendable (CloudScanProgress) -> Void = { _ in }
    ) throws -> CloudScanResult {
        var progressState = CloudScanProgress()
        let rootSnapshot = try reader.snapshot(for: root)
        progressState = CloudScanProgress(processedItems: 1, discoveredItems: 1, currentURL: root)
        progress(progressState)
        if rootSnapshot.isSymbolicLink {
            return CloudScanResult(items: [])
        }

        if !rootSnapshot.isDirectory {
            return CloudScanResult(items: [CloudItem(snapshot: rootSnapshot)])
        }

        var items: [CloudItem] = []
        try visitDirectory(
            root,
            recursive: recursive,
            into: &items,
            control: control,
            progressState: &progressState,
            progress: progress
        )
        return CloudScanResult(items: items)
    }

    private func visitDirectory(
        _ directory: URL,
        recursive: Bool,
        into items: inout [CloudItem],
        control: CloudOperationControl,
        progressState: inout CloudScanProgress,
        progress: @escaping @Sendable (CloudScanProgress) -> Void
    ) throws {
        let children = try reader.children(of: directory)
        progressState = CloudScanProgress(
            processedItems: progressState.processedItems,
            discoveredItems: progressState.discoveredItems + children.count,
            currentURL: directory
        )
        progress(progressState)

        for child in children {
            try control.waitIfNeeded()
            let snapshot = try reader.snapshot(for: child)
            progressState = CloudScanProgress(
                processedItems: progressState.processedItems + 1,
                discoveredItems: progressState.discoveredItems,
                currentURL: child
            )
            progress(progressState)
            if snapshot.isSymbolicLink {
                continue
            }

            if snapshot.isDirectory {
                if recursive {
                    try visitDirectory(
                        child,
                        recursive: true,
                        into: &items,
                        control: control,
                        progressState: &progressState,
                        progress: progress
                    )
                }
                continue
            }

            items.append(CloudItem(snapshot: snapshot))
        }
    }
}

private struct FoundationCloudResourceReader: CloudResourceReading {
    private static let resourceKeys: Set<URLResourceKey> = [
        .isDirectoryKey,
        .isSymbolicLinkKey,
        .isUbiquitousItemKey,
        .ubiquitousItemIsDownloadingKey,
        .ubiquitousItemIsUploadingKey,
        .ubiquitousItemIsExcludedFromSyncKey,
        .fileSizeKey,
        .fileAllocatedSizeKey,
    ]

    func snapshot(for url: URL) throws -> CloudItemSnapshot {
        let values = try url.resourceValues(forKeys: Self.resourceKeys)
        return CloudItemSnapshot(
            url: url,
            isDirectory: values.isDirectory ?? false,
            isSymbolicLink: values.isSymbolicLink ?? false,
            isUbiquitous: values.isUbiquitousItem ?? false,
            isDownloading: values.ubiquitousItemIsDownloading ?? false,
            isUploading: values.ubiquitousItemIsUploading ?? false,
            isExcludedFromSync: values.ubiquitousItemIsExcludedFromSync ?? false,
            isPinned: PinnedItemReader.isPinned(url),
            logicalSize: Int64(values.fileSize ?? 0),
            allocatedSize: Int64(values.fileAllocatedSize ?? 0)
        )
    }

    func children(of url: URL) throws -> [URL] {
        try FileManager.default.contentsOfDirectory(
            at: url,
            includingPropertiesForKeys: Array(Self.resourceKeys),
            options: [.skipsHiddenFiles]
        )
    }
}

private enum PinnedItemReader {
    private static let attribute = "com.apple.fileprovider.pinned#PX"

    static func isPinned(_ url: URL) -> Bool {
        getxattr(url.path, attribute, nil, 0, 0, 0) >= 0
    }
}
