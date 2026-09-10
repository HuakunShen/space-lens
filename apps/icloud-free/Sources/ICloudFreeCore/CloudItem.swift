import Foundation

public enum CloudItemStatus: String, Codable, Comparable, Sendable {
    case local
    case cloudOnly
    case downloading
    case uploading
    case excluded
    case pinned
    case notICloud
    case symlink
    case directory
    case unknown

    public static func < (lhs: CloudItemStatus, rhs: CloudItemStatus) -> Bool {
        lhs.rawValue < rhs.rawValue
    }
}

public struct CloudItemSnapshot: Sendable {
    public let url: URL
    public let isDirectory: Bool
    public let isSymbolicLink: Bool
    public let isUbiquitous: Bool
    public let isDownloading: Bool
    public let isUploading: Bool
    public let isExcludedFromSync: Bool
    public let isPinned: Bool
    public let logicalSize: Int64
    public let allocatedSize: Int64

    public init(
        url: URL,
        isDirectory: Bool,
        isSymbolicLink: Bool,
        isUbiquitous: Bool,
        isDownloading: Bool,
        isUploading: Bool,
        isExcludedFromSync: Bool,
        isPinned: Bool,
        logicalSize: Int64,
        allocatedSize: Int64
    ) {
        self.url = url
        self.isDirectory = isDirectory
        self.isSymbolicLink = isSymbolicLink
        self.isUbiquitous = isUbiquitous
        self.isDownloading = isDownloading
        self.isUploading = isUploading
        self.isExcludedFromSync = isExcludedFromSync
        self.isPinned = isPinned
        self.logicalSize = logicalSize
        self.allocatedSize = allocatedSize
    }
}

public struct CloudItem: Identifiable, Codable, Sendable {
    public let id: String
    public let url: URL
    public let name: String
    public let isDirectory: Bool
    public let isUbiquitous: Bool
    public let isPinned: Bool
    public let logicalSize: Int64
    public let allocatedSize: Int64
    public let status: CloudItemStatus

    public init(snapshot: CloudItemSnapshot) {
        id = snapshot.url.path
        url = snapshot.url
        name = snapshot.url.lastPathComponent
        isDirectory = snapshot.isDirectory
        isUbiquitous = snapshot.isUbiquitous
        isPinned = snapshot.isPinned
        logicalSize = snapshot.logicalSize
        allocatedSize = snapshot.allocatedSize

        if snapshot.isSymbolicLink {
            status = .symlink
        } else if snapshot.isDirectory {
            status = .directory
        } else if !snapshot.isUbiquitous {
            status = .notICloud
        } else if snapshot.isExcludedFromSync {
            status = .excluded
        } else if snapshot.isDownloading {
            status = .downloading
        } else if snapshot.isUploading {
            status = .uploading
        } else if snapshot.isPinned {
            status = .pinned
        } else if snapshot.logicalSize > 0 && snapshot.allocatedSize == 0 {
            status = .cloudOnly
        } else if snapshot.isUbiquitous {
            status = .local
        } else {
            status = .unknown
        }
    }

    public var isEvictable: Bool {
        status == .local
            && isUbiquitous
            && !isDirectory
            && !isPinned
            && allocatedSize > 0
    }

    public var reclaimableBytes: Int64 {
        isEvictable ? allocatedSize : 0
    }
}
