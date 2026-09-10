import Foundation

public struct CloudScanResult: Codable, Sendable {
    public let items: [CloudItem]
    public let totalFiles: Int
    public let localFiles: Int
    public let cloudOnlyFiles: Int
    public let syncingFiles: Int
    public let skippedFiles: Int
    public let logicalBytes: Int64
    public let allocatedBytes: Int64
    public let reclaimableBytes: Int64

    public init(items: [CloudItem]) {
        self.items = items
        totalFiles = items.count
        localFiles = items.filter { $0.status == .local }.count
        cloudOnlyFiles = items.filter { $0.status == .cloudOnly }.count
        syncingFiles = items.filter { $0.status == .downloading || $0.status == .uploading }.count
        skippedFiles = totalFiles - localFiles - cloudOnlyFiles - syncingFiles
        logicalBytes = items.reduce(0) { $0 + $1.logicalSize }
        allocatedBytes = items.reduce(0) { $0 + $1.allocatedSize }
        reclaimableBytes = items.reduce(0) { $0 + $1.reclaimableBytes }
    }
}

