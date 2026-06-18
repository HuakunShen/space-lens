/**
 * Kunkun custom-view adapter for SpaceLensAPI.
 *
 * The Svelte UI talks only to SpaceLensAPI; this file owns the Kunkun host
 * transport, backend process relay, permission prompts, and host-mediated trash
 * boundary. It mirrors @kunkunsh/api/ui/custom connection discovery without
 * making the standalone web app depend on the Kunkun API package.
 */
import { RPCChannel } from "kkrpc/streaming";
import {
  electronIpcTransport,
  type ElectronMessageEndpoint,
} from "kkrpc/electron";
import { webSocketClientTransport } from "kkrpc/ws";
import type {
  CleanupOutcome,
  ExecuteCleanupOptions,
  RemovalEntry,
  ScanTarget,
  SpaceLensAPI,
  StartScanOptions,
} from "./types";

type KunkunBackendAPI = SpaceLensAPI;

export interface KunkunHostAPI {
  backend: {
    spawn(
      scriptPath: string,
      runtime?: "node" | "bun" | "deno",
      options?: { fsReadAllow?: readonly string[] },
    ): Promise<{ backendId: string; channel: string }>;
    kill(backendId: string): Promise<void>;
  };
  path: {
    homeDir(): Promise<string>;
    desktopDir(): Promise<string>;
    downloadDir(): Promise<string>;
    documentDir(): Promise<string>;
  };
  permissions: {
    check(permissionType: string, scopePattern: string): Promise<boolean>;
    request(
      permissionType: string,
      scopePattern: string,
      reason?: string,
    ): Promise<boolean>;
  };
  system: {
    showInFinder(path: string): Promise<void>;
    trash(path: string | string[]): Promise<void>;
  };
  ui: {
    confirmAlert(options: {
      title: string;
      message?: string;
      primaryAction?: { title: string };
    }): Promise<boolean>;
  };
}

interface KunkunPluginAPI {
  onEvent(event: unknown, payload: unknown): Promise<void>;
  initialize(): Promise<void>;
  updateProps(): Promise<void>;
  executeHandler(): Promise<void>;
  destroy(): Promise<void>;
  syncTree(): Promise<void>;
}

interface KunkunHostRuntime {
  host: KunkunHostAPI;
  endpoint?: ElectronMessageEndpoint;
  hostChannel: RPCChannel<KunkunPluginAPI, KunkunHostAPI>;
}

interface KunkunBackendRuntime {
  backend: KunkunBackendAPI;
  backendId: string;
  fsReadAllow: readonly string[];
  backendChannel: RPCChannel<object, KunkunBackendAPI>;
  hostRuntime: KunkunHostRuntime;
  destroy(): Promise<void>;
}

export interface KunkunClientHostRuntime {
  readonly host: KunkunHostAPI;
}

export interface KunkunClientBackendRuntime {
  readonly backend: KunkunBackendAPI;
  readonly fsReadAllow: readonly string[];
  destroy(): Promise<void>;
}

export interface KunkunClientRuntime {
  getHostRuntime(): Promise<KunkunClientHostRuntime>;
  createBackendRuntime(
    fsReadAllow?: readonly string[],
  ): Promise<KunkunClientBackendRuntime>;
}

type KunkunHostConnection =
  | {
      kind: "electron";
      pluginId: string;
      channel: string;
      endpoint: ElectronMessageEndpoint;
    }
  | {
      kind: "websocket";
      pluginId: string;
      commandName: string;
      url: string;
    };

interface KunkunBootConfig {
  pluginId: string;
  commandName: string;
  wsRpcUrl: string;
}

let hostRuntimePromise: Promise<KunkunHostRuntime> | null = null;

export function createKunkunClient(): SpaceLensAPI {
  return createKunkunClientWithRuntime({
    getHostRuntime,
    createBackendRuntime,
  });
}

export function resetKunkunClientForTests(): void {
  hostRuntimePromise = null;
}

export function createKunkunClientWithRuntime(
  runtime: KunkunClientRuntime,
): SpaceLensAPI {
  let backendRuntimePromise: Promise<KunkunClientBackendRuntime> | null = null;
  const scanReadRootsById = new Map<string, readonly string[]>();

  async function getRuntime(
    fsReadAllow: readonly string[] = [],
  ): Promise<KunkunClientBackendRuntime> {
    const existing = backendRuntimePromise ? await backendRuntimePromise : null;
    if (existing && hasAllReadRoots(existing.fsReadAllow, fsReadAllow)) {
      return existing;
    }
    if (existing) {
      await existing.destroy();
      backendRuntimePromise = null;
    }
    backendRuntimePromise = runtime.createBackendRuntime(fsReadAllow);
    return backendRuntimePromise;
  }

  async function ensureReadAccess(options: StartScanOptions): Promise<void> {
    const { host } = await runtime.getHostRuntime();
    for (const path of options.paths) {
      const scope = recursiveScope(path);
      const allowed = await host.permissions.check("fs-read", scope);
      if (allowed) continue;
      const granted = await host.permissions.request(
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
    host: KunkunHostAPI,
    entries: RemovalEntry[],
  ): Promise<void> {
    for (const entry of entries) {
      const scope = recursiveScope(entry.path);
      const allowed = await host.permissions.check("fs-write", scope);
      if (allowed) continue;
      const granted = await host.permissions.request(
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
    const host = (await runtime.getHostRuntime()).host;
    const plan = await backendRuntime.backend.planCleanup(options);
    if (plan.entries.length === 0) {
      return { removed: [], bytesRemoved: 0, errors: plan.errors };
    }
    const confirmed = await host.ui.confirmAlert({
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
    await ensureWriteAccess(host, plan.entries);
    await host.system.trash(plan.entries.map((entry) => entry.path));
    return {
      removed: plan.entries,
      bytesRemoved: plan.totalSize,
      errors: plan.errors,
    };
  }

  return {
    async getScanTargets() {
      return getKunkunScanTargets((await runtime.getHostRuntime()).host);
    },

    async startScan(options) {
      await ensureReadAccess(options);
      const scan = await (
        await getRuntime(options.paths)
      ).backend.startScan(options);
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
      return (await runtime.getHostRuntime()).host.system.showInFinder(path);
    },
  };

  function cleanupReadRoots(options: ExecuteCleanupOptions): readonly string[] {
    return (
      scanReadRootsById.get(options.scanId) ??
      uniqueReadRoots(options.entries.map((entry) => parentPath(entry.path)))
    );
  }
}

async function getKunkunScanTargets(
  host: KunkunHostAPI,
): Promise<ScanTarget[]> {
  const home = await host.path.homeDir();
  const optionalTargets = await Promise.allSettled([
    host.path.desktopDir(),
    host.path.downloadDir(),
    host.path.documentDir(),
  ]);
  const targets: ScanTarget[] = [
    {
      id: "home",
      label: "Home",
      path: home,
      kind: "folder",
      description: home,
      size: 0,
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
    });
  });
  return uniqueScanTargets(targets);
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

async function createBackendRuntime(
  fsReadAllow: readonly string[] = [],
): Promise<KunkunBackendRuntime> {
  const hostRuntime = await getHostRuntime();
  const readRoots = uniqueReadRoots(fsReadAllow);
  const { backendId, channel } = await hostRuntime.host.backend.spawn(
    "$EXTENSION/dist/backend.js",
    "node",
    readRoots.length > 0 ? { fsReadAllow: readRoots } : undefined,
  );
  if (!hostRuntime.endpoint) {
    throw new Error(
      "Kunkun backend relay is not available for browser WebSocket custom-view hosts yet.",
    );
  }
  const backendChannel = new RPCChannel<object, KunkunBackendAPI>(
    electronIpcTransport({ endpoint: hostRuntime.endpoint, channel }),
  );
  return {
    backend: backendChannel.getAPI(),
    backendId,
    fsReadAllow: readRoots,
    backendChannel,
    hostRuntime,
    async destroy() {
      backendChannel.destroy();
      await hostRuntime.host.backend.kill(backendId);
    },
  };
}

async function getHostRuntime(): Promise<KunkunHostRuntime> {
  hostRuntimePromise ??= createHostRuntime();
  return hostRuntimePromise;
}

async function createHostRuntime(): Promise<KunkunHostRuntime> {
  const connection = await resolveKunkunHostConnection();
  const hostChannel = new RPCChannel<KunkunPluginAPI, KunkunHostAPI>(
    connection.kind === "electron"
      ? electronIpcTransport({
          endpoint: connection.endpoint,
          channel: connection.channel,
        })
      : webSocketClientTransport({ url: connection.url }),
    { expose: createPluginAPI() },
  );
  return {
    ...(connection.kind === "electron"
      ? { endpoint: connection.endpoint }
      : {}),
    host: hostChannel.getAPI(),
    hostChannel,
  };
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

function createPluginAPI(): KunkunPluginAPI {
  return {
    async onEvent() {},
    async initialize() {},
    async updateProps() {},
    async executeHandler() {},
    async destroy() {},
    async syncTree() {},
  };
}

function getElectronIpcEndpoint(): ElectronMessageEndpoint {
  const endpoint = (
    globalThis as {
      window?: { electron?: { ipcRenderer?: ElectronMessageEndpoint } };
    }
  ).window?.electron?.ipcRenderer;
  if (!endpoint) {
    throw new Error("Kunkun Electron IPC endpoint is not available");
  }
  return endpoint;
}

async function resolveKunkunHostConnection(): Promise<KunkunHostConnection> {
  const connection = resolveKunkunConnection();
  if (connection.kind !== "websocket") return connection;

  const bootConfig = await fetchBootConfig();
  if (!bootConfig) return connection;
  return {
    kind: "websocket",
    pluginId: bootConfig.pluginId,
    commandName: bootConfig.commandName,
    url: withPluginQuery(
      bootConfig.wsRpcUrl,
      bootConfig.pluginId,
      bootConfig.commandName,
    ),
  };
}

function resolveKunkunConnection(): KunkunHostConnection {
  const location = globalThis.window?.location;
  const params = new URLSearchParams(location?.search ?? "");
  const pluginId = resolvePluginId(location, params);
  const commandName = resolveCommandName(location, params);
  const explicitWebSocketUrl =
    params.get("kunkun_ws_rpc_url") ?? params.get("kunkun_rpc_url");

  if (explicitWebSocketUrl) {
    return {
      kind: "websocket",
      pluginId,
      commandName,
      url: withPluginQuery(explicitWebSocketUrl, pluginId, commandName),
    };
  }

  const routeWebSocketUrl = resolveRouteWebSocketUrl(
    location,
    pluginId,
    commandName,
  );
  if (routeWebSocketUrl) {
    return {
      kind: "websocket",
      pluginId,
      commandName,
      url: routeWebSocketUrl,
    };
  }

  const endpoint = getElectronIpcEndpoint();
  return {
    kind: "electron",
    pluginId,
    channel: `kkrpc-plugin-${pluginId}`,
    endpoint,
  };
}

function resolvePluginId(
  location: Location | undefined,
  params: URLSearchParams,
): string {
  const fromQuery = params.get("kkrpc_plugin_id") ?? params.get("pluginId");
  if (fromQuery) return fromQuery;
  const fromPath = parseCustomViewPath(location?.pathname).pluginId;
  if (fromPath) return fromPath;
  return location?.hostname ?? "unknown";
}

function resolveCommandName(
  location: Location | undefined,
  params: URLSearchParams,
): string {
  const fromQuery = params.get("commandName");
  if (fromQuery) return fromQuery;
  return parseCustomViewPath(location?.pathname).commandName ?? "";
}

function parseCustomViewPath(pathname: string | undefined): {
  pluginId?: string;
  commandName?: string;
} {
  const parts = (pathname ?? "").split("/").filter(Boolean);
  if (parts[0] !== "custom-views" || !parts[1] || !parts[2]) {
    return {};
  }
  return {
    pluginId: safeDecode(parts[1]),
    commandName: safeDecode(parts[2]),
  };
}

function resolveRouteWebSocketUrl(
  location: Location | undefined,
  pluginId: string,
  commandName: string,
): string | null {
  const parsed = parseCustomViewPath(location?.pathname);
  if (!parsed.pluginId || !parsed.commandName || !location?.host) {
    return null;
  }
  const protocol = location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${location.host}/rpc?pluginId=${encodeURIComponent(pluginId)}&commandName=${encodeURIComponent(commandName)}`;
}

async function fetchBootConfig(): Promise<KunkunBootConfig | null> {
  const location = globalThis.window?.location;
  const bootUrl = resolveBootConfigUrl(location);
  if (!bootUrl) return null;
  const response = await fetch(bootUrl);
  if (!response.ok) return null;
  const body: unknown = await response.json();
  if (!isBootConfig(body)) return null;
  return body;
}

function resolveBootConfigUrl(location: Location | undefined): string | null {
  const parsed = parseCustomViewPath(location?.pathname);
  if (
    !parsed.pluginId ||
    !parsed.commandName ||
    !location?.protocol ||
    !location.host
  ) {
    return null;
  }
  const url = new URL(`${location.protocol}//${location.host}`);
  url.pathname = `/custom-views/${encodeURIComponent(parsed.pluginId)}/${encodeURIComponent(parsed.commandName)}/kunkun.boot.json`;
  const bootToken = new URLSearchParams(location.search ?? "").get("bootToken");
  if (bootToken) {
    url.searchParams.set("bootToken", bootToken);
  }
  return url.toString();
}

function isBootConfig(value: unknown): value is KunkunBootConfig {
  if (typeof value !== "object" || value === null) return false;
  const record = value as Record<string, unknown>;
  return (
    typeof record["pluginId"] === "string" &&
    typeof record["commandName"] === "string" &&
    typeof record["wsRpcUrl"] === "string"
  );
}

function withPluginQuery(
  url: string,
  pluginId: string,
  commandName: string,
): string {
  const parsed = new URL(url);
  if (!parsed.searchParams.has("pluginId")) {
    parsed.searchParams.set("pluginId", pluginId);
  }
  if (commandName && !parsed.searchParams.has("commandName")) {
    parsed.searchParams.set("commandName", commandName);
  }
  return parsed.toString();
}

function safeDecode(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return "";
  }
}
