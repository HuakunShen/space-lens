import Foundation
import SpaceLensFFIBridge

struct RustScanResult: @unchecked Sendable {
    let root: SpaceLensNode
    let json: Data
}

enum RustScanError: LocalizedError {
    case unavailable
    case invalidSnapshot

    var errorDescription: String? {
        switch self {
        case .unavailable:
            "The scanner could not be loaded or the selected folder could not be read."
        case .invalidSnapshot:
            "The scanner returned an invalid filesystem snapshot."
        }
    }
}

enum RustScanClient {
    static func scan(path: String) throws -> RustScanResult {
        let buffer = path.withCString { sl_scan_directory_json($0) }
        guard let pointer = buffer.ptr, buffer.len > 0 else {
            throw RustScanError.unavailable
        }
        let data = Data(bytes: pointer, count: Int(buffer.len))
        sl_free_buffer(buffer)

        let snapshot = try JSONDecoder().decode(RustSnapshot.self, from: data)
        let baseURL = URL(fileURLWithPath: path, isDirectory: true).standardizedFileURL
        guard let root = snapshot.rootNode(baseURL: baseURL) else {
            throw RustScanError.invalidSnapshot
        }
        return RustScanResult(root: root, json: data)
    }
}

private struct RustSnapshot: Decodable {
    let roots: [RustSnapshotRoot]
    let nodes: [RustSnapshotNode]

    func rootNode(baseURL: URL) -> SpaceLensNode? {
        guard roots.first != nil,
              let rootNode = nodes.first(where: { $0.parentID == nil }) else {
            return nil
        }
        let nodesByParent = Dictionary(grouping: nodes.compactMap { node -> (String, RustSnapshotNode)? in
            guard let parentID = node.parentID else { return nil }
            return (parentID, node)
        }, by: { $0.0 })
        return makeNode(rootNode, childrenByParent: nodesByParent, baseURL: baseURL)
    }

    private func makeNode(
        _ node: RustSnapshotNode,
        childrenByParent: [String: [(String, RustSnapshotNode)]],
        baseURL: URL
    ) -> SpaceLensNode {
        let children = (childrenByParent[node.id] ?? [])
            .map(\.1)
            .map { makeNode($0, childrenByParent: childrenByParent, baseURL: baseURL) }
        let allocated = UInt64(node.allocatedBytes ?? "0") ?? 0
        let logical = UInt64(node.logicalBytes ?? "0") ?? allocated
        let kind: SpaceLensNodeKind
        switch node.kind {
        case "root": kind = .volume
        case "directory": kind = .folder
        case "file": kind = .file
        default: kind = .other
        }
        return SpaceLensNode(
            id: node.id,
            name: node.name ?? "Unnamed",
            kind: kind,
            allocatedBytes: allocated,
            logicalBytes: logical,
            children: children,
            sourceURL: safeURL(relativePath: node.relativePath, baseURL: baseURL)
        )
    }

    private func safeURL(relativePath: String?, baseURL: URL) -> URL? {
        guard let relativePath, !relativePath.isEmpty else { return nil }
        let candidate = (relativePath == "."
            ? baseURL
            : baseURL.appendingPathComponent(relativePath)).standardizedFileURL
        let basePath = baseURL.path
        guard candidate.path == basePath || candidate.path.hasPrefix(basePath + "/") else {
            return nil
        }
        return candidate
    }
}

private struct RustSnapshotRoot: Decodable {
    let id: String
}

private struct RustSnapshotNode: Decodable {
    let id: String
    let parentID: String?
    let name: String?
    let kind: String
    let logicalBytes: String?
    let allocatedBytes: String?
    let relativePath: String?

    enum CodingKeys: String, CodingKey {
        case id
        case parentID = "parentId"
        case name
        case kind
        case logicalBytes
        case allocatedBytes
        case relativePath
    }
}
