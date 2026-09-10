import Foundation

public enum CloudPathResolver {
    public static let iCloudDriveRoot = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Mobile Documents/com~apple~CloudDocs", isDirectory: true)

    public static func resolve(_ input: String) -> URL {
        if input == "icloud" || input == "icloud:" {
            return iCloudDriveRoot
        }

        if input.hasPrefix("icloud:") {
            let relative = String(input.dropFirst("icloud:".count))
            return iCloudDriveRoot.appendingPathComponent(relative)
        }

        if input == "~" || input.hasPrefix("~/") {
            let suffix = input == "~" ? "" : String(input.dropFirst(2))
            return FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(suffix)
        }

        if input.hasPrefix("/") {
            return URL(fileURLWithPath: input)
        }

        return URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
            .appendingPathComponent(input)
    }

    public static func resolveExisting(_ input: String) throws -> URL {
        let url = resolve(input).standardizedFileURL
        guard FileManager.default.fileExists(atPath: url.path) else {
            throw CloudPathError.notFound(url.path)
        }
        return url
    }
}

public enum CloudPathError: LocalizedError, Sendable {
    case notFound(String)

    public var errorDescription: String? {
        switch self {
        case .notFound(let path):
            return "Path does not exist: \(path)"
        }
    }
}

