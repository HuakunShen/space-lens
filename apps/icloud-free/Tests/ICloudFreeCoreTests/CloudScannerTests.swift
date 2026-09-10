import XCTest
@testable import ICloudFreeCore

final class CloudScannerTests: XCTestCase {
    func testRecursiveScanIncludesFilesAndNeverFollowsSymlinks() throws {
        let root = URL(fileURLWithPath: "/tmp/lightroom")
        let local = root.appendingPathComponent("local.dng")
        let cloud = root.appendingPathComponent("cloud.dng")
        let link = root.appendingPathComponent("linked.dng")
        let nested = root.appendingPathComponent("nested")
        let nestedLocal = nested.appendingPathComponent("nested.dng")

        let reader = FakeCloudResourceReader(
            snapshots: [
                root: snapshot(root, directory: true, ubiquitous: true),
                local: snapshot(local, logical: 100, allocated: 100),
                cloud: snapshot(cloud, logical: 300, allocated: 0),
                link: snapshot(link, logical: 100, allocated: 100, symbolicLink: true),
                nested: snapshot(nested, directory: true, ubiquitous: true),
                nestedLocal: snapshot(nestedLocal, logical: 50, allocated: 50),
            ],
            children: [
                root: [local, cloud, link, nested],
                nested: [nestedLocal],
            ]
        )

        let result = try CloudScanner(reader: reader).scan(root: root, recursive: true)

        XCTAssertEqual(result.items.map(\.name), ["local.dng", "cloud.dng", "nested.dng"])
        XCTAssertEqual(result.reclaimableBytes, 150)
    }

    private func snapshot(
        _ url: URL,
        directory: Bool = false,
        logical: Int64 = 0,
        allocated: Int64 = 0,
        ubiquitous: Bool = true,
        symbolicLink: Bool = false
    ) -> CloudItemSnapshot {
        CloudItemSnapshot(
            url: url,
            isDirectory: directory,
            isSymbolicLink: symbolicLink,
            isUbiquitous: ubiquitous,
            isDownloading: false,
            isUploading: false,
            isExcludedFromSync: false,
            isPinned: false,
            logicalSize: logical,
            allocatedSize: allocated
        )
    }
}

private final class FakeCloudResourceReader: CloudResourceReading, @unchecked Sendable {
    let snapshots: [URL: CloudItemSnapshot]
    let children: [URL: [URL]]

    init(snapshots: [URL: CloudItemSnapshot], children: [URL: [URL]]) {
        self.snapshots = snapshots
        self.children = children
    }

    func snapshot(for url: URL) throws -> CloudItemSnapshot {
        guard let snapshot = snapshots[url] else {
            throw CocoaError(.fileNoSuchFile)
        }
        return snapshot
    }

    func children(of url: URL) throws -> [URL] {
        children[url] ?? []
    }
}
