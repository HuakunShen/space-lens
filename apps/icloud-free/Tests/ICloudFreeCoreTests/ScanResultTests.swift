import XCTest
@testable import ICloudFreeCore

final class ScanResultTests: XCTestCase {
    func testSummarySeparatesLocalCloudOnlyAndSkippedItems() {
        let result = CloudScanResult(items: [
            item(name: "local.dng", logical: 100, allocated: 100),
            item(name: "cloud.dng", logical: 300, allocated: 0),
            item(name: "ordinary.txt", logical: 50, allocated: 50, ubiquitous: false),
        ])

        XCTAssertEqual(result.totalFiles, 3)
        XCTAssertEqual(result.localFiles, 1)
        XCTAssertEqual(result.cloudOnlyFiles, 1)
        XCTAssertEqual(result.skippedFiles, 1)
        XCTAssertEqual(result.logicalBytes, 450)
        XCTAssertEqual(result.allocatedBytes, 150)
        XCTAssertEqual(result.reclaimableBytes, 100)
    }

    private func item(
        name: String,
        logical: Int64,
        allocated: Int64,
        ubiquitous: Bool = true
    ) -> CloudItem {
        CloudItem(snapshot: CloudItemSnapshot(
            url: URL(fileURLWithPath: "/tmp/\(name)"),
            isDirectory: false,
            isSymbolicLink: false,
            isUbiquitous: ubiquitous,
            isDownloading: false,
            isUploading: false,
            isExcludedFromSync: false,
            isPinned: false,
            logicalSize: logical,
            allocatedSize: allocated
        ))
    }
}

