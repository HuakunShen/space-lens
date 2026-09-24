import SwiftUI

enum SpaceLensNodeKind: String, CaseIterable, Codable {
    case volume
    case folder
    case file
    case other
}

struct SpaceLensNode: Identifiable, Hashable {
    let id: String
    let name: String
    let kind: SpaceLensNodeKind
    let allocatedBytes: UInt64
    let logicalBytes: UInt64
    let children: [SpaceLensNode]
    let sourceURL: URL?

    init(
        id: String,
        name: String,
        kind: SpaceLensNodeKind,
        allocatedBytes: UInt64,
        logicalBytes: UInt64,
        children: [SpaceLensNode],
        sourceURL: URL? = nil
    ) {
        self.id = id
        self.name = name
        self.kind = kind
        self.allocatedBytes = allocatedBytes
        self.logicalBytes = logicalBytes
        self.children = children
        self.sourceURL = sourceURL
    }

    var totalBytes: UInt64 { allocatedBytes }

    var descendantCount: Int {
        1 + children.reduce(0) { $0 + $1.descendantCount }
    }

    func node(withID nodeID: String?) -> SpaceLensNode? {
        guard let nodeID else { return nil }
        if id == nodeID { return self }
        for child in children {
            if let match = child.node(withID: nodeID) {
                return match
            }
        }
        return nil
    }

    static let demoRoot = SpaceLensNode(
        id: "root",
        name: "Macintosh HD",
        kind: .volume,
        allocatedBytes: 690_000_000_000,
        logicalBytes: 734_000_000_000,
        children: [
            folder("Users", 422_000_000_000, [
                folder("hk", 286_000_000_000, [
                    folder("Dev", 124_000_000_000, [
                        folder("space-lens", 36_000_000_000, [
                            folder("target", 20_000_000_000),
                            folder("apps", 9_000_000_000),
                            folder("packages", 7_000_000_000),
                        ]),
                        folder("kunkun", 42_000_000_000),
                        folder("xross", 31_000_000_000),
                    ]),
                    folder("Library", 83_000_000_000),
                    folder("Downloads", 45_000_000_000),
                ]),
                folder("Shared", 52_000_000_000),
            ]),
            folder("System", 76_700_000_000),
            folder("Applications", 59_000_000_000),
            folder("private", 30_400_000_000),
            folder("Library", 24_100_000_000),
            folder("opt", 22_600_000_000),
            folder("Other", 1_000_000_000),
        ]
    )

    static func folder(_ name: String, _ bytes: UInt64, _ children: [SpaceLensNode] = []) -> SpaceLensNode {
        SpaceLensNode(
            id: name.lowercased().replacingOccurrences(of: " ", with: "-"),
            name: name,
            kind: .folder,
            allocatedBytes: bytes,
            logicalBytes: bytes,
            children: children
        )
    }

    static func makeLargeFixture(nodeCount: Int) -> SpaceLensNode {
        let leafCount = max(1, nodeCount - 1)
        let groupSize = 100
        let groupCount = Int(ceil(Double(leafCount) / Double(groupSize)))
        let groups = (0..<groupCount).map { groupIndex in
            let start = groupIndex * groupSize
            let end = min(start + groupSize, leafCount)
            var leaves: [SpaceLensNode] = []
            leaves.reserveCapacity(end - start)
            for leafIndex in start..<end {
                let bytes = UInt64((leafIndex % 97 + 1) * 4096)
                leaves.append(SpaceLensNode(
                    id: "file-" + String(leafIndex),
                    name: "file-" + String(leafIndex) + ".bin",
                    kind: .file,
                    allocatedBytes: bytes,
                    logicalBytes: bytes,
                    children: []
                ))
            }
            return SpaceLensNode(
                id: "group-" + String(groupIndex),
                name: "Group " + String(groupIndex),
                kind: .folder,
                allocatedBytes: leaves.reduce(0) { $0 + $1.allocatedBytes },
                logicalBytes: leaves.reduce(0) { $0 + $1.logicalBytes },
                children: leaves
            )
        }
        return SpaceLensNode(
            id: "large-root",
            name: "Synthetic Volume",
            kind: .volume,
            allocatedBytes: groups.reduce(0) { $0 + $1.allocatedBytes },
            logicalBytes: groups.reduce(0) { $0 + $1.logicalBytes },
            children: groups
        )
    }
}

extension SpaceLensNode: @unchecked Sendable {}

enum SidebarSection: Hashable {
    case home
    case map
    case recent
    case collections
    case iCloud
    case exports
}

extension SidebarSection {
    var title: String {
        switch self {
        case .home: "Home"
        case .map: "Space Map"
        case .recent: "Recent Scans"
        case .collections: "Collections"
        case .iCloud: "iCloud Local Copies"
        case .exports: "Exports"
        }
    }

    var icon: String {
        switch self {
        case .home: "house"
        case .map: "circle.grid.3x3.fill"
        case .recent: "clock"
        case .collections: "tray.full"
        case .iCloud: "icloud"
        case .exports: "arrow.up.doc"
        }
    }
}
