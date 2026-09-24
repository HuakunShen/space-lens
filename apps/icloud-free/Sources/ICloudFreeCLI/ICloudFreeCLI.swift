import ArgumentParser
import Foundation
import ICloudFreeCore

public struct ICloudFreeCLI: ParsableCommand {
    public static let configuration = CommandConfiguration(
        commandName: "icloud-free",
        abstract: "Inspect iCloud Drive local copies and safely free disk space.",
        discussion: """
        Scanning is read-only. Eviction is a dry-run unless --execute is supplied.

        Examples:
          icloud-free status icloud:Pictures/lightroom --recursive
          icloud-free evict icloud:Pictures/lightroom --recursive --dry-run
          icloud-free evict icloud:Pictures/lightroom --recursive --execute
        """,
        subcommands: [StatusCommand.self, EvictCommand.self]
    )

    public init() {}
}

public struct StatusCommand: ParsableCommand {
    public static let configuration = CommandConfiguration(
        commandName: "status",
        abstract: "Show local, cloud-only, and reclaimable iCloud files."
    )

    @Argument(help: "File or folder to inspect. Use icloud: for iCloud Drive paths.")
    public var path: String = "."

    @Flag(name: .shortAndLong, help: "Recursively inspect folders.")
    public var recursive = false

    @Flag(name: .shortAndLong, help: "Emit sorted JSON output.")
    public var json = false

    public init() {}

    public func run() async throws {
        let url = try CloudPathResolver.resolveExisting(path)
        let result = try CloudScanner().scan(root: url, recursive: recursive)

        if json {
            try CLIOutput.printJSON(result)
        } else {
            CLIOutput.printSummary(result, path: url)
            CLIOutput.printItems(result.items.sorted { $0.reclaimableBytes > $1.reclaimableBytes })
        }
    }
}

public struct EvictCommand: ParsableCommand {
    public static let configuration = CommandConfiguration(
        commandName: "evict",
        abstract: "Preview or evict local copies without deleting iCloud items."
    )

    @Argument(help: "File or folder to inspect.")
    public var path: String? = nil

    @Flag(name: .shortAndLong, help: "Recursively inspect folders.")
    public var recursive = false

    @Flag(help: "Preview only. This is the default behavior.")
    public var dryRun = false

    @Flag(help: "Actually remove local copies. Required to change disk state.")
    public var execute = false

    @Flag(name: .shortAndLong, help: "Emit JSON output.")
    public var json = false

    public init() {}

    public mutating func validate() throws {
        guard let path, !path.isEmpty else {
            throw ValidationError("Please provide a file or folder path.")
        }
        if dryRun && execute {
            throw ValidationError("Choose either --dry-run or --execute, not both.")
        }
    }

    public func run() async throws {
        guard let path else {
            throw ValidationError("Please provide a file or folder path.")
        }
        let url = try CloudPathResolver.resolveExisting(path)
        var isDirectory: ObjCBool = false
        _ = FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory)
        if isDirectory.boolValue && !recursive {
            throw ValidationError("\(url.lastPathComponent) is a folder; use --recursive.")
        }

        let result = try CloudScanner().scan(root: url, recursive: recursive)
        let service = EvictionService()
        let plan = service.plan(for: result)
        let shouldDryRun = !execute
        let report = try await service.execute(plan, dryRun: shouldDryRun)

        if json {
            try CLIOutput.printJSON(report)
        } else {
            if shouldDryRun {
                print("Dry run: \(report.wouldEvictCount) files would free \(CLIOutput.humanSize(plan.totalBytes)).")
            } else {
                print("Evicted \(report.evictedCount) files, freed \(CLIOutput.humanSize(report.freedBytes)).")
                if report.failedCount > 0 {
                    print("Failed: \(report.failedCount)")
                }
            }
        }
    }
}

private enum CLIOutput {
    static func humanSize(_ bytes: Int64) -> String {
        ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
    }

    static func printSummary(_ result: CloudScanResult, path: URL) {
        print(path.path)
        print("Files: \(result.totalFiles)")
        print("Local: \(result.localFiles)")
        print("Cloud only: \(result.cloudOnlyFiles)")
        print("Syncing: \(result.syncingFiles)")
        print("Skipped: \(result.skippedFiles)")
        print("Logical size: \(humanSize(result.logicalBytes))")
        print("Allocated: \(humanSize(result.allocatedBytes))")
        print("Reclaimable: \(humanSize(result.reclaimableBytes))")
    }

    static func printItems(_ items: [CloudItem]) {
        for item in items where item.isEvictable || item.status == .cloudOnly {
            print("\(item.status.rawValue)\t\(humanSize(item.reclaimableBytes))\t\(item.url.path)")
        }
    }

    static func printJSON<T: Encodable>(_ value: T) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let data = try encoder.encode(value)
        print(String(decoding: data, as: UTF8.self))
    }
}
