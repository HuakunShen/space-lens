import XCTest
@testable import ICloudFreeAppSupport
@testable import ICloudFreeCore

@MainActor
final class AppModelTests: XCTestCase {
    func testInitialStateDoesNotOfferEviction() {
        let model = AppModel()

        XCTAssertNil(model.scanResult)
        XCTAssertFalse(model.canEvict)
        XCTAssertFalse(model.isShowingConfirmation)
    }

    func testScanPublishesReclaimableSummaryAndConfirmationGate() async throws {
        let root = URL(fileURLWithPath: "/tmp/lightroom")
        let local = root.appendingPathComponent("local.dng")
        let reader = AppModelTestReader(
            snapshots: [
                root: snapshot(root, directory: true),
                local: snapshot(local, logical: 100, allocated: 100),
            ],
            children: [root: [local]]
        )
        let model = AppModel(scanner: CloudScanner(reader: reader))

        await model.scan(root: root)
        model.requestEvictionConfirmation()

        XCTAssertEqual(model.scanResult?.reclaimableBytes, 100)
        XCTAssertTrue(model.canEvict)
        XCTAssertTrue(model.isShowingConfirmation)
    }

    private func snapshot(_ url: URL, directory: Bool = false, logical: Int64 = 0, allocated: Int64 = 0) -> CloudItemSnapshot {
        CloudItemSnapshot(
            url: url,
            isDirectory: directory,
            isSymbolicLink: false,
            isUbiquitous: true,
            isDownloading: false,
            isUploading: false,
            isExcludedFromSync: false,
            isPinned: false,
            logicalSize: logical,
            allocatedSize: allocated
        )
    }
}

private final class AppModelTestReader: CloudResourceReading, @unchecked Sendable {
    let snapshots: [URL: CloudItemSnapshot]
    let children: [URL: [URL]]

    init(snapshots: [URL: CloudItemSnapshot], children: [URL: [URL]]) {
        self.snapshots = snapshots
        self.children = children
    }

    func snapshot(for url: URL) throws -> CloudItemSnapshot {
        guard let snapshot = snapshots[url] else { throw CocoaError(.fileNoSuchFile) }
        return snapshot
    }

    func children(of url: URL) throws -> [URL] {
        children[url] ?? []
    }
}

