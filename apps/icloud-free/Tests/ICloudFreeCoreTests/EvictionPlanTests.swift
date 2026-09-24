import XCTest
@testable import ICloudFreeCore

final class EvictionPlanTests: XCTestCase {
    func testPlanContainsOnlyEvictableLocalFiles() {
        let result = CloudScanResult(items: [
            item(name: "local.dng", logical: 100, allocated: 100),
            item(name: "cloud.dng", logical: 300, allocated: 0),
            item(name: "pinned.dng", logical: 200, allocated: 200, pinned: true),
        ])

        let plan = EvictionService().plan(for: result)

        XCTAssertEqual(plan.entries.map(\.name), ["local.dng"])
        XCTAssertEqual(plan.totalBytes, 100)
    }

    func testDryRunDoesNotCallEvictor() async throws {
        let item = item(name: "local.dng", logical: 100, allocated: 100)
        let plan = EvictionPlan(entries: [EvictionEntry(item: item)])
        let evictor = RecordingEvictor()

        let report = try await EvictionService().execute(plan, dryRun: true, evictor: evictor)

        XCTAssertEqual(report.wouldEvictCount, 1)
        XCTAssertTrue(evictor.urls.isEmpty)
    }

    func testExecutionReportsProgressForEachEntry() async throws {
        let first = item(name: "first.dng", logical: 100, allocated: 100)
        let second = item(name: "second.dng", logical: 200, allocated: 200)
        let plan = EvictionPlan(entries: [EvictionEntry(item: first), EvictionEntry(item: second)])
        let evictor = RecordingEvictor()
        let controller = CloudOperationControl()
        let progress = ProgressRecorder()

        let report = try await EvictionService().execute(
            plan,
            dryRun: false,
            evictor: evictor,
            control: controller,
            progress: { progress.append($0) }
        )

        XCTAssertEqual(progress.values.map(\.processedEntries), [1, 2])
        XCTAssertEqual(progress.values.last?.totalEntries, 2)
        XCTAssertEqual(report.evictedCount, 2)
    }

    func testExecutionUsesBoundedConcurrency() async throws {
        let entries = (0..<24).map { index in
            EvictionEntry(item: item(name: "file-\(index).dng", logical: 100, allocated: 100))
        }
        let evictor = ConcurrentRecordingEvictor(delayNanoseconds: 15_000_000)
        let report = try await EvictionService(maxConcurrency: 4).execute(
            EvictionPlan(entries: entries),
            dryRun: false,
            evictor: evictor
        )

        XCTAssertEqual(report.evictedCount, entries.count)
        XCTAssertGreaterThan(evictor.maximumActive, 1)
        XCTAssertLessThanOrEqual(evictor.maximumActive, 4)
    }

    func testOperationControlPausesAndResumesWorker() {
        let controller = CloudOperationControl()
        controller.pause()
        let gate = DispatchSemaphore(value: 0)

        DispatchQueue.global().async {
            try? controller.waitIfNeeded()
            gate.signal()
        }

        XCTAssertEqual(gate.wait(timeout: .now() + 0.05), .timedOut)
        controller.resume()
        XCTAssertEqual(gate.wait(timeout: .now() + 0.5), .success)
    }

    func testOperationControlCancelsWorker() {
        let controller = CloudOperationControl()
        controller.cancel()

        XCTAssertThrowsError(try controller.waitIfNeeded()) { error in
            XCTAssertEqual(error as? CloudOperationError, .cancelled)
        }
    }

    private func item(name: String, logical: Int64, allocated: Int64, pinned: Bool = false) -> CloudItem {
        CloudItem(snapshot: CloudItemSnapshot(
            url: URL(fileURLWithPath: "/tmp/\(name)"),
            isDirectory: false,
            isSymbolicLink: false,
            isUbiquitous: true,
            isDownloading: false,
            isUploading: false,
            isExcludedFromSync: false,
            isPinned: pinned,
            logicalSize: logical,
            allocatedSize: allocated
        ))
    }
}

private final class RecordingEvictor: CloudEvicting, @unchecked Sendable {
    private(set) var urls: [URL] = []

    func evict(_ url: URL) throws {
        urls.append(url)
    }
}

private final class ProgressRecorder: @unchecked Sendable {
    private let lock = NSLock()
    private var storage: [EvictionProgress] = []

    var values: [EvictionProgress] {
        lock.lock()
        defer { lock.unlock() }
        return storage
    }

    func append(_ value: EvictionProgress) {
        lock.lock()
        storage.append(value)
        lock.unlock()
    }
}

private final class ConcurrentRecordingEvictor: CloudEvicting, @unchecked Sendable {
    private let lock = NSLock()
    private let delayNanoseconds: UInt64
    private var active = 0
    private(set) var maximumActive = 0

    init(delayNanoseconds: UInt64) {
        self.delayNanoseconds = delayNanoseconds
    }

    func evict(_ url: URL) throws {
        lock.lock()
        active += 1
        maximumActive = max(maximumActive, active)
        lock.unlock()

        Thread.sleep(forTimeInterval: Double(delayNanoseconds) / 1_000_000_000)

        lock.lock()
        active -= 1
        lock.unlock()
    }
}
