import type {
  ChildrenPage,
  CleanupOutcome,
  CleanupPlanOptions,
  ExecuteCleanupOptions,
  GetChildrenRequest,
  GetNodeRequest,
  RemovalPlan,
  ScanTarget,
  ScanSession,
  ScanStatus,
  SpaceLensAPI,
  StartScanOptions,
  TreeNodeSummary,
  TreeSlice,
  TreeSliceNode,
} from "./types";

interface DemoNode {
  id: string;
  name: string;
  path: string;
  size: number;
  depth: number;
  ignored: boolean;
  children: DemoNode[];
  parentId: string | null;
}

interface DemoSession {
  session: ScanSession;
  root: DemoNode;
  nodes: Map<string, DemoNode>;
}

const GiB = 1024 ** 3;

export function createDemoClient(): SpaceLensAPI {
  const sessions = new Map<string, DemoSession>();

  return {
    async getScanTargets() {
      return demoScanTargets();
    },

    async startScan(options) {
      const demo = createDemoSession(options);
      sessions.set(demo.session.scanId, demo);
      return demo.session;
    },

    async getNode(request) {
      const demo = getSession(sessions, request.scanId);
      const node = demo.nodes.get(request.nodeId);
      if (!node) throw new Error(`Unknown node ${request.nodeId}`);
      return createSlice(demo, node, request.depth, request.maxChildrenPerNode);
    },

    async getChildren(request) {
      const demo = getSession(sessions, request.scanId);
      return createChildrenPage(demo, request);
    },

    async getScanStatus(scanId) {
      const demo = getSession(sessions, scanId);
      return {
        scanId,
        state: "ready",
        message: "Demo scan ready",
        progress: 1,
        currentPath: demo.root.path,
        bytesScanned: demo.root.size,
        entriesScanned: demo.nodes.size,
        rootIds: demo.session.rootIds,
        label: demo.session.label,
        updatedAt: new Date().toISOString(),
      };
    },

    async cancelScan() {
      return;
    },

    async planCleanup(options) {
      return planFromCollector(options);
    },

    async executeCleanup(options) {
      const plan = planFromCollector(options);
      return {
        removed: [],
        bytesRemoved: 0,
        errors: ["Demo mode does not delete files."],
      };
    },
  };
}

function getSession(
  sessions: Map<string, DemoSession>,
  scanId: string,
): DemoSession {
  const session = sessions.get(scanId);
  if (!session) throw new Error(`Unknown scan ${scanId}`);
  return session;
}

function createDemoSession(options: StartScanOptions): DemoSession {
  const scanId = `demo-${Date.now().toString(36)}`;
  const root = createRoot(options.paths);
  const nodes = new Map<string, DemoNode>();
  indexNode(root, nodes);
  const session: ScanSession = {
    scanId,
    rootIds: [root.id],
    createdAt: new Date().toISOString(),
    label: root.name,
  };
  return { session, root, nodes };
}

function createRoot(paths: string[]): DemoNode {
  if (paths.length > 1) {
    const children = paths.map((path, index) =>
      createFolderTree(
        `selected-${index}`,
        basename(path),
        path,
        1,
        62 * GiB + index * 17 * GiB,
      ),
    );
    return attachParents({
      id: "root",
      name: "Selected Folders",
      path: "selected://folders",
      size: sum(children),
      depth: 0,
      ignored: false,
      children,
      parentId: null,
    });
  }

  const input = paths[0] ?? "/Demo/Projects";
  if (input === "/" || input.toLowerCase().includes("portable")) {
    return attachParents(createVolumeTree());
  }
  return attachParents(
    createFolderTree("dev", basename(input) || "Dev", input, 0, 265.6 * GiB),
  );
}

function demoScanTargets(): ScanTarget[] {
  return [
    {
      id: "demo-volume",
      label: "Demo Volume",
      path: "/Demo",
      kind: "volume",
      description: "/Demo",
      size: 0,
    },
    {
      id: "demo-projects",
      label: "Projects",
      path: "/Demo/Projects",
      kind: "folder",
      description: "/Demo/Projects",
      size: 0,
    },
    {
      id: "demo-archives",
      label: "Archives",
      path: "/Demo/Archives",
      kind: "folder",
      description: "/Demo/Archives",
      size: 0,
    },
  ];
}

function createVolumeTree(): DemoNode {
  return {
    id: "portable",
    name: "Portable2TB",
    path: "/Volumes/Portable2TB",
    size: 166.3 * GiB,
    depth: 0,
    ignored: false,
    parentId: null,
    children: [
      createFolderTree(
        "extdev",
        "ExtDev",
        "/Volumes/Portable2TB/ExtDev",
        1,
        123.5 * GiB,
      ),
      createFolderTree(
        "movies",
        "Movies",
        "/Volumes/Portable2TB/Movies",
        1,
        37.2 * GiB,
      ),
      createFolderTree(
        "small",
        "small files",
        "/Volumes/Portable2TB/small files",
        1,
        3.6 * GiB,
      ),
      createFolderTree(
        "hidden",
        "hidden cache",
        "/Volumes/Portable2TB/.cache",
        1,
        2 * GiB,
        true,
      ),
    ],
  };
}

function createFolderTree(
  id: string,
  name: string,
  path: string,
  depth: number,
  size: number,
  ignored = false,
): DemoNode {
  const labels = [
    "CrossCopy",
    "kunkun",
    "bitlake",
    "tauri-demo",
    "kkrpc",
    "uniview",
    "maokong",
    "quant",
    "videos",
    "polyinsight",
    "polymarket-quant",
    "polyquant",
    "kunkun.worktrees",
    "MyWeb",
    "blog",
  ];
  if (depth >= 4 || size < 1.2 * GiB) {
    return {
      id,
      name,
      path,
      size,
      depth,
      ignored,
      parentId: null,
      children: [],
    };
  }
  const childCount = Math.min(
    labels.length,
    Math.max(4, Math.floor(size / (12 * GiB))),
  );
  const children = Array.from({ length: childCount }, (_, index) => {
    const fraction = 0.24 / (index + 1) + 0.012;
    const childSize = Math.max(0.25 * GiB, size * fraction);
    const childName =
      labels[(index + depth) % labels.length] ?? `Folder ${index + 1}`;
    const childId = `${id}-${index}`;
    return createFolderTree(
      childId,
      childName,
      `${path}/${childName}`,
      depth + 1,
      childSize,
      ignored && index % 4 === 0,
    );
  });
  const used = sum(children);
  const free = Math.max(0, size - used);
  if (free > 0.4 * GiB) {
    children.push({
      id: `${id}-loose`,
      name: "free space / small files",
      path: `${path}/small-files`,
      size: free,
      depth: depth + 1,
      ignored: false,
      parentId: null,
      children: [],
    });
  }
  return { id, name, path, size, depth, ignored, parentId: null, children };
}

function attachParents(
  root: DemoNode,
  parentId: string | null = null,
): DemoNode {
  root.parentId = parentId;
  for (const child of root.children) {
    attachParents(child, root.id);
  }
  return root;
}

function indexNode(node: DemoNode, nodes: Map<string, DemoNode>): void {
  nodes.set(node.id, node);
  for (const child of node.children) indexNode(child, nodes);
}

function createSlice(
  demo: DemoSession,
  focus: DemoNode,
  depth: number,
  maxChildren: number,
): TreeSlice {
  const tree = sliceNode(focus, depth, maxChildren);
  const ancestors = ancestorsOf(demo, focus).map(toSummary);
  const children = sortedChildren(focus).slice(0, maxChildren).map(toSummary);
  const omitted = sortedChildren(focus).slice(maxChildren);
  return {
    scanId: demo.session.scanId,
    focusNode: toSummary(focus),
    ancestors,
    children,
    tree,
    totalSize: focus.size,
    loadedDepth: depth,
    maxDepth: depth,
    truncated: tree.truncated,
    omittedBytes: sum(omitted),
    omittedCount: omitted.length,
  };
}

function sliceNode(
  node: DemoNode,
  depth: number,
  maxChildren: number,
): TreeSliceNode {
  const children =
    depth <= 0
      ? []
      : sortedChildren(node)
          .slice(0, maxChildren)
          .map((child) => sliceNode(child, depth - 1, maxChildren));
  const omitted =
    depth <= 0 ? node.children : sortedChildren(node).slice(maxChildren);
  return {
    ...toSummary(node),
    loadedDepth: depth,
    truncated: omitted.length > 0 || children.some((child) => child.truncated),
    children,
    omittedBytes: sum(omitted),
    omittedCount: omitted.length,
  };
}

function createChildrenPage(
  demo: DemoSession,
  request: GetChildrenRequest,
): ChildrenPage {
  const node = demo.nodes.get(request.nodeId);
  if (!node) throw new Error(`Unknown node ${request.nodeId}`);
  const sorted = [...node.children].sort((left, right) =>
    request.sort === "name"
      ? left.name.localeCompare(right.name)
      : right.size - left.size || left.name.localeCompare(right.name),
  );
  return {
    scanId: request.scanId,
    nodeId: request.nodeId,
    items: sorted
      .slice(request.offset, request.offset + request.limit)
      .map(toSummary),
    offset: request.offset,
    limit: request.limit,
    total: sorted.length,
    sort: request.sort,
  };
}

function ancestorsOf(demo: DemoSession, node: DemoNode): DemoNode[] {
  const ancestors: DemoNode[] = [];
  let current: DemoNode | undefined = node;
  while (current) {
    ancestors.unshift(current);
    current = current.parentId ? demo.nodes.get(current.parentId) : undefined;
  }
  return ancestors;
}

function toSummary(node: DemoNode): TreeNodeSummary {
  return {
    id: node.id,
    name: node.name,
    path: node.path,
    size: Math.round(node.size),
    depth: node.depth,
    ignored: node.ignored,
    hasChildren: node.children.length > 0,
    childCount: node.children.length,
    loadedDepth: 0,
    truncated: false,
  };
}

function sortedChildren(node: DemoNode): DemoNode[] {
  return [...node.children].sort(
    (left, right) =>
      right.size - left.size || left.name.localeCompare(right.name),
  );
}

function sum(nodes: DemoNode[]): number {
  return nodes.reduce((total, node) => total + node.size, 0);
}

function basename(path: string): string {
  return path.split("/").filter(Boolean).at(-1) ?? path;
}

function planFromCollector(
  options: CleanupPlanOptions | ExecuteCleanupOptions,
): RemovalPlan {
  const entries = options.entries.map((entry) => ({
    path: entry.path,
    size: entry.size,
    reason: "Collector",
    preset: "manual",
  }));
  return {
    entries,
    totalSize: entries.reduce((total, entry) => total + entry.size, 0),
    errors: [],
  };
}
