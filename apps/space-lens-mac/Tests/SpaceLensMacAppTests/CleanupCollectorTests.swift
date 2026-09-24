import XCTest
@testable import SpaceLensMacApp

final class CleanupCollectorTests: XCTestCase {
    func testParentSelectionReplacesStagedDescendants() {
        let parentURL = URL(fileURLWithPath: "/tmp/space-lens-cache")
        let childURL = parentURL.appendingPathComponent("nested")
        let child = SpaceLensNode(
            id: "child",
            name: "nested",
            kind: .folder,
            allocatedBytes: 20,
            logicalBytes: 20,
            children: [],
            sourceURL: childURL
        )
        let parent = SpaceLensNode(
            id: "parent",
            name: "space-lens-cache",
            kind: .folder,
            allocatedBytes: 100,
            logicalBytes: 100,
            children: [child],
            sourceURL: parentURL
        )

        var collector = CleanupCollector()
        XCTAssertTrue(collector.add(node: child))
        XCTAssertFalse(collector.add(node: child))
        XCTAssertTrue(collector.add(node: parent))
        XCTAssertEqual(collector.entries.map(\.id), ["parent"])
        XCTAssertEqual(collector.totalBytes, 100)
    }

    func testSimilarPrefixIsNotTreatedAsDescendant() {
        let first = SpaceLensNode(
            id: "first",
            name: "cache",
            kind: .folder,
            allocatedBytes: 10,
            logicalBytes: 10,
            children: [],
            sourceURL: URL(fileURLWithPath: "/tmp/cache")
        )
        let second = SpaceLensNode(
            id: "second",
            name: "cache-archive",
            kind: .folder,
            allocatedBytes: 20,
            logicalBytes: 20,
            children: [],
            sourceURL: URL(fileURLWithPath: "/tmp/cache-archive")
        )

        var collector = CleanupCollector()
        XCTAssertTrue(collector.add(node: first))
        XCTAssertTrue(collector.add(node: second))
        XCTAssertEqual(Set(collector.entries.map(\.id)), ["first", "second"])
    }

    func testSyntheticNodeCannotBeCollected() {
        var collector = CleanupCollector()
        XCTAssertFalse(collector.add(node: SpaceLensNode(
            id: "synthetic",
            name: "Synthetic",
            kind: .folder,
            allocatedBytes: 10,
            logicalBytes: 10,
            children: []
        )))
        XCTAssertTrue(collector.entries.isEmpty)
    }
}
