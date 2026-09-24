import XCTest
@testable import SpaceLensMacApp

final class SunburstLayoutTests: XCTestCase {
    func testTopLevelSegmentsCoverFullCircle() {
        let segments = SunburstLayout.segments(for: SpaceLensNode.demoRoot)
        let topLevel = segments.filter { $0.depth == 0 }
        XCTAssertFalse(topLevel.isEmpty)
        XCTAssertEqual(topLevel.reduce(0) { $0 + $1.span }, 2 * Double.pi, accuracy: 0.000_001)
        XCTAssertTrue(segments.allSatisfy { $0.outerRadius > $0.innerRadius })
    }

    func testSmallChildrenAggregateIntoOther() {
        let root = SpaceLensNode(
            id: "root",
            name: "Root",
            kind: .volume,
            allocatedBytes: 100,
            logicalBytes: 100,
            children: (0..<5).map { index in
                SpaceLensNode(
                    id: "child-\(index)",
                    name: "Child \(index)",
                    kind: .folder,
                    allocatedBytes: 20,
                    logicalBytes: 20,
                    children: []
                )
            }
        )
        let segments = SunburstLayout.segments(for: root, maxChildrenPerNode: 3)
        XCTAssertTrue(segments.contains { $0.name == "Other" && $0.isOther })
        XCTAssertEqual(segments.filter { $0.depth == 0 }.count, 4)
    }

    func testLargeSyntheticFixtureIsBoundedForCanvasLayout() {
        let root = SpaceLensNode.makeLargeFixture(nodeCount: 100_000)
        XCTAssertGreaterThanOrEqual(root.descendantCount, 100_000)
        let segments = SunburstLayout.segments(for: root, maxDepth: 3)
        XCTAssertLessThan(segments.count, 2_000)
    }
}
