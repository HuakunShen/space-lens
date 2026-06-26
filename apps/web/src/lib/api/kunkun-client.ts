/**
 * Kunkun custom-view adapter for SpaceLensAPI.
 *
 * This file owns Space Lens behavior in Kunkun mode: path presets, permission
 * prompts, backend reuse, recent-scan storage, and host-mediated trash. Kunkun
 * host bootstrap is imported lazily so standalone mode never initializes the
 * custom-view kkrpc channel.
 */
import type {
  KunkunBackendConnection,
  KunkunRuntimeAdapter,
} from "./kunkun-runtime";
import type {
  CleanupOutcome,
  ExecuteCleanupOptions,
  RemovalEntry,
  ScanTarget,
  SpaceLensAPI,
  StartScanOptions,
} from "./types";

interface KunkunBackendRuntime {
  readonly backend: SpaceLensAPI;
  readonly approvedReadRoots: readonly string[];
  destroy(): Promise<void>;
}

interface RecentScanTargetEntry {
  path: string;
  label: string;
  lastScannedAt: string;
  scanCount: number;
}

type KunkunCustomApiModule = {
  readonly LocalStorage: KunkunRuntimeAdapter["storage"];
  confirmAlert: KunkunRuntimeAdapter["confirmAlert"];
  showInFinder: KunkunRuntimeAdapter["showInFinder"];
  trash: KunkunRuntimeAdapter["trash"];
  spawnBackend(options: {
    scriptPath: string;
    runtime?: "auto" | "node" | "bun" | "deno";
  }): Promise<KunkunBackendConnection<Record<string, (...args: unknown[]) => unknown>>>;
};

type KunkunApiModule = {
  readonly path: KunkunRuntimeAdapter["path"];
  readonly permissions: KunkunRuntimeAdapter["permissions"];
};

const BACKEND_SCRIPT_PATH = "$EXTENSION/dist/backend.js";
const BACKEND_RUNTIME = "deno" as const;
const RECENT_SCAN_TARGETS_STORAGE_KEY = "space-lens.recentScanTargets.v1";
const MAX_RECENT_SCAN_TARGETS = 12;
const kunkunCustomApiSpecifier: string = "@kunkunsh/sdk/ui/custom";
const kunkunApiSpecifier: string = "@kunkunsh/api";

let adapterPromise: Promise<KunkunRuntimeAdapter> | null = null;

export function createKunkunClient(): SpaceLensAPI {
  return createKunkunClientWithAdapterProvider(getDefaultAdapter);
}

export function resetKunkunClientForTests(): void {
  adapterPromise = null;
}

export function createKunkunClientWithAdapter(
  adapter: KunkunRuntimeAdapter,
): SpaceLensAPI {
  return createKunkunClientWithAdapterProvider(async () => adapter);
}

function createKunkunClientWithAdapterProvider(
  getAdapter: () => Promise<KunkunRuntimeAdapter>,
): SpaceLensAPI {
  let backendRuntimePromise: Promise<KunkunBackendRuntime> | null = null;
  const scanReadRootsById = new Map<string, readonly string[]>();

  async function getRuntime(
    approvedReadRoots: readonly string[] = [],
  ): Promise<KunkunBackendRuntime> {
    const existing = backendRuntimePromise ? await backendRuntimePromise : null;
    if (
      existing &&
      hasAllReadRoots(existing.approvedReadRoots, approvedReadRoots)
    ) {
      return existing;
    }
    if (existing) {
      await existing.destroy();
      backendRuntimePromise = null;
    }
    backendRuntimePromise = createBackendRuntime(
      await getAdapter(),
      approvedReadRoots,
    );
    return backendRuntimePromise;
  }

  async function ensureReadAccess(options: StartScanOptions): Promise<void> {
    const adapter = await getAdapter();
    for (const path of options.paths) {
      const scope = recursiveScope(path);
      const allowed = await adapter.permissions.check("fs-read", scope);
      if (allowed) continue;
      const granted = await adapter.permissions.request(
        "fs-read",
        scope,
        "Scan this folder with Space Lens.",
      );
      if (!granted) {
        throw new Error(`Space Lens does not have permission to scan ${path}`);
      }
    }
  }

  async function ensureWriteAccess(
    adapter: KunkunRuntimeAdapter,
    entries: RemovalEntry[],
  ): Promise<void> {
    for (const entry of entries) {
      const scope = recursiveScope(entry.path);
      const allowed = await adapter.permissions.check("fs-write", scope);
      if (allowed) continue;
      const granted = await adapter.permissions.request(
        "fs-write",
        scope,
        "Move this Space Lens cleanup item to Trash.",
      );
      if (!granted) {
        throw new Error(
          `Space Lens does not have permission to delete ${entry.path}`,
        );
      }
    }
  }

  async function executeWithHostConfirmation(
    options: ExecuteCleanupOptions,
  ): Promise<CleanupOutcome> {
    const backendRuntime = await getRuntime(cleanupReadRoots(options));
    const adapter = await getAdapter();
    const plan = await backendRuntime.backend.planCleanup(options);
    if (plan.entries.length === 0) {
      return { removed: [], bytesRemoved: 0, errors: plan.errors };
    }
    const confirmed = await adapter.confirmAlert({
      title: "Move selected items to Trash?",
      message: `${plan.entries.length} items will be moved to Trash. This cannot be undone from Space Lens.`,
      primaryAction: { title: "Move to Trash" },
    });
    if (!confirmed) {
      return {
        removed: [],
        bytesRemoved: 0,
        errors: ["Deletion cancelled."],
      };
    }
    await ensureWriteAccess(adapter, plan.entries);
    await adapter.trash(plan.entries.map((entry) => entry.path));
    return {
      removed: plan.entries,
      bytesRemoved: plan.totalSize,
      errors: plan.errors,
    };
  }

  return {
    async getScanTargets() {
      return getKunkunScanTargets(await getAdapter());
    },
    async startScan(options) {
      await ensureReadAccess(options);
      const scan = await (
        await getRuntime(options.paths)
      ).backend.startScan(options);
      await recordRecentScanTargets(await getAdapter(), options.paths);
      scanReadRootsById.set(scan.scanId, uniqueReadRoots(options.paths));
      return scan;
    },
    async getNode(request) {
      return (await getRuntime()).backend.getNode(request);
    },
    async getChildren(request) {
      return (await getRuntime()).backend.getChildren(request);
    },
    async getScanStatus(scanId) {
      return (await getRuntime()).backend.getScanStatus(scanId);
    },
    async cancelScan(scanId) {
      return (await getRuntime()).backend.cancelScan(scanId);
    },
    async planCleanup(options) {
      return (await getRuntime(cleanupReadRoots(options))).backend.planCleanup(
        options,
      );
    },
    async executeCleanup(options) {
      return executeWithHostConfirmation(options);
    },
    async showInFileManager(path) {
      return (await getAdapter()).showInFinder(path);
    },
    async forgetScanTarget(path) {
      return forgetRecentScanTarget(await getAdapter(), path);
    },
  };

  function cleanupReadRoots(options: ExecuteCleanupOptions): readonly string[] {
    return (
      scanReadRootsById.get(options.scanId) ??
      uniqueReadRoots(options.entries.map((entry) => parentPath(entry.path)))
    );
  }
}

async function getDefaultAdapter(): Promise<KunkunRuntimeAdapter> {
  adapterPromise ??= createDefaultKunkunRuntimeAdapter();
  return adapterPromise;
}

async function createDefaultKunkunRuntimeAdapter(): Promise<KunkunRuntimeAdapter> {
  const [customApi, api] = await Promise.all([
    import(kunkunCustomApiSpecifier) as Promise<KunkunCustomApiModule>,
    import(kunkunApiSpecifier) as Promise<KunkunApiModule>,
  ]);
  const {
    LocalStorage,
    confirmAlert,
    showInFinder,
    spawnBackend,
    trash,
  } = customApi;
  const { path, permissions } = api;

  return {
    path,
    permissions,
    storage: {
      getItem: (key) => LocalStorage.getItem(key),
      setItem: (key, value) => LocalStorage.setItem(key, value),
      removeItem: (key) => LocalStorage.removeItem(key),
    },
    confirmAlert,
    showInFinder,
    trash,
    spawnBackend: async (options) => {
      const connection = await spawnBackend(options);
      return {
        api: connection.api as unknown as SpaceLensAPI,
        backendId: connection.backendId,
        destroy: connection.destroy,
      } satisfies KunkunBackendConnection<SpaceLensAPI>;
    },
  };
}

async function createBackendRuntime(
  adapter: KunkunRuntimeAdapter,
  approvedReadRoots: readonly string[] = [],
): Promise<KunkunBackendRuntime> {
  const connection: KunkunBackendConnection<SpaceLensAPI> =
    await adapter.spawnBackend({
      scriptPath: BACKEND_SCRIPT_PATH,
      runtime: BACKEND_RUNTIME,
    });
  return {
    backend: connection.api,
    approvedReadRoots: uniqueReadRoots(approvedReadRoots),
    async destroy() {
      await connection.destroy();
    },
  };
}

async function getKunkunScanTargets(
  adapter: KunkunRuntimeAdapter,
): Promise<ScanTarget[]> {
  const home = await adapter.path.homeDir();
  const optionalTargets = await Promise.allSettled([
    adapter.path.desktopDir(),
    adapter.path.downloadDir(),
    adapter.path.documentDir(),
  ]);
  const targets: ScanTarget[] = [
    {
      id: "home",
      label: "Home",
      path: home,
      kind: "folder",
      description: home,
      size: 0,
      source: "preset",
    },
  ];
  const labels = ["Desktop", "Downloads", "Documents"];
  optionalTargets.forEach((result, index) => {
    if (result.status !== "fulfilled" || !result.value) return;
    targets.push({
      id: labels[index]?.toLowerCase() ?? `folder-${index}`,
      label: labels[index] ?? "Folder",
      path: result.value,
      kind: "folder",
      description: result.value,
      size: 0,
      source: "preset",
    });
  });
  return uniqueScanTargets([
    ...(await getRecentScanTargets(adapter)),
    ...targets,
  ]);
}

async function getRecentScanTargets(
  adapter: KunkunRuntimeAdapter,
): Promise<ScanTarget[]> {
  const entries = await readRecentScanTargets(adapter);
  return entries.map((entry) => ({
    id: `recent:${entry.path}`,
    label: entry.label,
    path: entry.path,
    kind: "folder",
    description: entry.path,
    size: 0,
    source: "recent",
    removable: true,
    lastScannedAt: entry.lastScannedAt,
  }));
}

async function recordRecentScanTargets(
  adapter: KunkunRuntimeAdapter,
  paths: readonly string[],
): Promise<void> {
  const existing = await readRecentScanTargets(adapter);
  const entries = [...existing];
  const now = new Date().toISOString();

  for (const inputPath of paths) {
    const path = normalizeFsPath(inputPath);
    if (!path) continue;
    const existingIndex = entries.findIndex((entry) => entry.path === path);
    const previous =
      existingIndex >= 0 ? entries.splice(existingIndex, 1)[0] : undefined;
    entries.unshift({
      path,
      label: previous?.label ?? labelFromPath(path),
      lastScannedAt: now,
      scanCount: (previous?.scanCount ?? 0) + 1,
    });
  }

  await writeRecentScanTargets(
    adapter,
    entries.slice(0, MAX_RECENT_SCAN_TARGETS),
  );
}

async function forgetRecentScanTarget(
  adapter: KunkunRuntimeAdapter,
  inputPath: string,
): Promise<void> {
  const path = normalizeFsPath(inputPath);
  if (!path) return;
  const entries = (await readRecentScanTargets(adapter)).filter(
    (entry) => entry.path !== path,
  );
  await writeRecentScanTargets(adapter, entries);
}

async function readRecentScanTargets(
  adapter: KunkunRuntimeAdapter,
): Promise<RecentScanTargetEntry[]> {
  const raw = await adapter.storage.getItem(RECENT_SCAN_TARGETS_STORAGE_KEY);
  if (!raw) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    const entries: RecentScanTargetEntry[] = [];
    for (const item of parsed) {
      const entry = parseRecentScanTargetEntry(item);
      if (entry) entries.push(entry);
    }
    return sortRecentScanTargets(dedupeRecentScanTargets(entries)).slice(
      0,
      MAX_RECENT_SCAN_TARGETS,
    );
  } catch {
    return [];
  }
}

async function writeRecentScanTargets(
  adapter: KunkunRuntimeAdapter,
  entries: readonly RecentScanTargetEntry[],
): Promise<void> {
  await adapter.storage.setItem(
    RECENT_SCAN_TARGETS_STORAGE_KEY,
    JSON.stringify(dedupeRecentScanTargets([...entries])),
  );
}

function parseRecentScanTargetEntry(
  value: unknown,
): RecentScanTargetEntry | null {
  if (!value || typeof value !== "object") return null;
  const record = value as Record<string, unknown>;
  const path =
    typeof record.path === "string" ? normalizeFsPath(record.path) : "";
  const lastScannedAt =
    typeof record.lastScannedAt === "string" &&
    !Number.isNaN(Date.parse(record.lastScannedAt))
      ? record.lastScannedAt
      : "";
  if (!path || !lastScannedAt) return null;
  const label =
    typeof record.label === "string" && record.label.trim()
      ? record.label.trim()
      : labelFromPath(path);
  const scanCount =
    typeof record.scanCount === "number" && Number.isFinite(record.scanCount)
      ? Math.max(1, Math.floor(record.scanCount))
      : 1;
  return {
    path,
    label,
    lastScannedAt,
    scanCount,
  };
}

function dedupeRecentScanTargets(
  entries: readonly RecentScanTargetEntry[],
): RecentScanTargetEntry[] {
  const seen = new Set<string>();
  const result: RecentScanTargetEntry[] = [];
  for (const entry of entries) {
    const path = normalizeFsPath(entry.path);
    if (!path || seen.has(path)) continue;
    seen.add(path);
    result.push({ ...entry, path });
  }
  return result;
}

function sortRecentScanTargets(
  entries: readonly RecentScanTargetEntry[],
): RecentScanTargetEntry[] {
  return [...entries].sort(
    (left, right) =>
      Date.parse(right.lastScannedAt) - Date.parse(left.lastScannedAt),
  );
}

function labelFromPath(path: string): string {
  const normalized = normalizeFsPath(path);
  const parts = normalized.split(/[/\\]/).filter(Boolean);
  return parts.at(-1) ?? normalized;
}

function uniqueScanTargets(targets: ScanTarget[]): ScanTarget[] {
  const seen = new Set<string>();
  return targets.filter((target) => {
    const key = normalizeFsPath(target.path);
    if (!key || seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function recursiveScope(path: string): string {
  return path.endsWith("/**") ? path : `${path.replace(/\/$/, "")}/**`;
}

function hasAllReadRoots(
  existing: readonly string[],
  requested: readonly string[],
): boolean {
  const existingRoots = uniqueReadRoots(existing);
  return uniqueReadRoots(requested).every((root) =>
    existingRoots.some((existingRoot) => readRootCovers(existingRoot, root)),
  );
}

function uniqueReadRoots(roots: readonly string[]): string[] {
  const seen = new Set<string>();
  for (const root of roots) {
    const normalized = normalizeFsPath(root);
    if (normalized) seen.add(normalized);
  }
  return [...seen];
}

function readRootCovers(existingRoot: string, requestedRoot: string): boolean {
  const existing = normalizeFsPath(existingRoot);
  const requested = normalizeFsPath(requestedRoot);
  if (existing === requested) return true;
  return (
    requested.startsWith(`${existing}/`) ||
    requested.startsWith(`${existing}\\`)
  );
}

function parentPath(input: string): string {
  const normalized = normalizeFsPath(input);
  const slashIndex = normalized.lastIndexOf("/");
  const backslashIndex = normalized.lastIndexOf("\\");
  const index = Math.max(slashIndex, backslashIndex);
  if (index <= 0) return normalized;
  if (index === 2 && /^[A-Za-z]:/.test(normalized))
    return normalized.slice(0, 3);
  return normalized.slice(0, index);
}

function normalizeFsPath(input: string): string {
  let normalized = input.trim();
  while (normalized.length > 1 && /[/\\]$/.test(normalized)) {
    normalized = normalized.slice(0, -1);
  }
  return normalized;
}
