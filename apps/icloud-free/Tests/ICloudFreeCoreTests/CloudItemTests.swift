import XCTest
@testable import ICloudFreeCore

final class CloudItemTests: XCTestCase {
    func testLocalUbiquitousFileIsEvictable() {
        let item = CloudItem(snapshot: snapshot(logical: 120, allocated: 120))

        XCTAssertEqual(item.status, .local)
        XCTAssertTrue(item.isEvictable)
        XCTAssertEqual(item.reclaimableBytes, 120)
    }

    func testDatalessFileIsCloudOnlyEvenWhenFoundationReportsLocal() {
        let item = CloudItem(snapshot: snapshot(logical: 120, allocated: 0))

        XCTAssertEqual(item.status, .cloudOnly)
        XCTAssertFalse(item.isEvictable)
        XCTAssertEqual(item.reclaimableBytes, 0)
    }

    func testPinnedFileIsNotEvictable() {
        let item = CloudItem(snapshot: snapshot(logical: 120, allocated: 120, pinned: true))

        XCTAssertEqual(item.status, .pinned)
        XCTAssertFalse(item.isEvictable)
    }

    func testNonUbiquitousAndSymlinkItemsAreSkipped() {
        let ordinary = CloudItem(snapshot: snapshot(logical: 120, allocated: 120, ubiquitous: false))
        let symlink = CloudItem(snapshot: snapshot(logical: 120, allocated: 120, symbolicLink: true))

        XCTAssertEqual(ordinary.status, .notICloud)
        XCTAssertEqual(symlink.status, .symlink)
        XCTAssertFalse(ordinary.isEvictable)
        XCTAssertFalse(symlink.isEvictable)
    }

    private func snapshot(
        logical: Int64,
        allocated: Int64,
        ubiquitous: Bool = true,
        symbolicLink: Bool = false,
        pinned: Bool = false
    ) -> CloudItemSnapshot {
        CloudItemSnapshot(
            url: URL(fileURLWithPath: "/tmp/lightroom/photo.dng"),
            isDirectory: false,
            isSymbolicLink: symbolicLink,
            isUbiquitous: ubiquitous,
            isDownloading: false,
            isUploading: false,
            isExcludedFromSync: false,
            isPinned: pinned,
            logicalSize: logical,
            allocatedSize: allocated
        )
    }
}

