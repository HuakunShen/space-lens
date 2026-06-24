/**
 * Smoke tests for the Kunkun SpaceLensAPI adapter.
 *
 * These tests inject the high-level Kunkun runtime adapter that the plugin
 * package owns, so the shared web app never needs to construct kkrpc channels.
 */
import { describe, expect, test } from "bun:test";
import { createKunkunClientWithAdapter } from "../src/lib/api/kunkun-client";
import type {
  KunkunBackendConnection,
  KunkunRuntimeAdapter,
} from "../src/lib/api/kunkun-runtime";
import type {
  CleanupPlanOptions,
  ExecuteCleanupOptions,
  GetChildrenRequest,
  GetNodeRequest,
  RemovalPlan,
  SpaceLensAPI,
  StartScanOptions,
} from "../src/lib/api/types";

function startOptions(paths: string[]): StartScanOptions {
  return {
    paths,
    ignoreHidden: true,
    respectGitignore: true,
    ignoredMode: "summarize",
    initialDepth: 2,
    maxChildrenPerNode: 50,
  };
}

function cleanupOptions(
  path = "/tmp/space-lens/old.log",
): ExecuteCleanupOptions {
  return {
    scanId: "scan-1",
    entries: [
      {
        id: "collector-1",
        scanId: "scan-1",
        nodeId: "node-1",
        path,
        name: "old.log",
        size: 123,
        addedAt: "2026-06-17T00:00:00.000Z",
      },
    ],
  };
}

function removalPlan(path = "/tmp/space-lens/old.log"): RemovalPlan {
  return {
    entries: [
      {
        path,
        size: 123,
        reason: "manual",
        preset: "collector",
      },
    ],
    totalSize: 123,
    errors: [],
  };
}

function createFakeBackend(plan = removalPlan()): SpaceLensAPI & {
  nativeDeleteCalls: ExecuteCleanupOptions[];
  cleanupPlans: CleanupPlanOptions[];
  scans: StartScanOptions[];
} {
  const nativeDeleteCalls: ExecuteCleanupOptions[] = [];
  const cleanupPlans: CleanupPlanOptions[] = [];
  const scans: StartScanOptions[] = [];
  let scanCounter = 0;
  return {
    nativeDeleteCalls,
    cleanupPlans,
    scans,
    async getScanTargets() {
      return [];
    },
    async startScan(options) {
      const scanId = `scan-${++scanCounter}`;
      scans.push(options);
      return {
        scanId,
        rootIds: ["root-1"],
        createdAt: "2026-06-17T00:00:00.000Z",
        label: options.paths.join(", "),
      };
    },
    async getNode(_request: GetNodeRequest) {
      throw new Error("not used");
    },
    async getChildren(_request: GetChildrenRequest) {
      throw new Error("not used");
    },
    async getScanStatus(scanId) {
      return {
        scanId,
        state: "ready",
        message: "ready",
        progress: 1,
        currentPath: null,
        bytesScanned: 0,
        entriesScanned: 0,
        rootIds: ["root-1"],
        label: "ready",
        updatedAt: "2026-06-17T00:00:00.000Z",
      };
    },
    async cancelScan() {},
    async planCleanup(options) {
      cleanupPlans.push(options);
      return plan;
    },
    async executeCleanup(options) {
      nativeDeleteCalls.push(options);
      return {
        removed: plan.entries,
        bytesRemoved: plan.totalSize,
        errors: plan.errors,
      };
    },
  };
}

function createHarness(
  options: {
    readAllowed?: boolean;
    writeAllowed?: boolean;
    confirmDelete?: boolean;
    plan?: RemovalPlan;
    storage?: Record<string, string>;
  } = {},
): {
  api: SpaceLensAPI;
  backend: ReturnType<typeof createFakeBackend>;
  spawnedBackends: readonly {
    scriptPath: string;
    runtime?: "auto" | "node" | "bun" | "deno";
  }[];
  destroyedBackends: readonly string[];
  permissionChecks: readonly { permissionType: string; scopePattern: string }[];
  permissionRequests: readonly {
    permissionType: string;
    scopePattern: string;
    reason?: string;
  }[];
  trashCalls: readonly (string | string[])[];
  pathCalls: readonly string[];
  storage: Record<string, string>;
} {
  const backend = createFakeBackend(options.plan);
  const spawnedBackends: Array<{
    scriptPath: string;
    runtime?: "auto" | "node" | "bun" | "deno";
  }> = [];
  const destroyedBackends: string[] = [];
  const permissionChecks: Array<{
    permissionType: string;
    scopePattern: string;
  }> = [];
  const permissionRequests: Array<{
    permissionType: string;
    scopePattern: string;
    reason?: string;
  }> = [];
  const trashCalls: Array<string | string[]> = [];
  const pathCalls: string[] = [];
  const storage = options.storage ?? {};
  let backendCounter = 0;

  const adapter: KunkunRuntimeAdapter = {
    path: {
      async homeDir() {
        pathCalls.push("homeDir");
        return "/Users/tester";
      },
      async desktopDir() {
        pathCalls.push("desktopDir");
        return "/Users/tester/Desktop";
      },
      async downloadDir() {
        pathCalls.push("downloadDir");
        return "/Users/tester/Downloads";
      },
      async documentDir() {
        pathCalls.push("documentDir");
        return "/Users/tester/Documents";
      },
    },
    permissions: {
      async check(permissionType, scopePattern) {
        permissionChecks.push({ permissionType, scopePattern });
        if (permissionType === "fs-read") return options.readAllowed ?? false;
        if (permissionType === "fs-write") return options.writeAllowed ?? false;
        return false;
      },
      async request(permissionType, scopePattern, reason) {
        permissionRequests.push({
          permissionType,
          scopePattern,
          ...(reason ? { reason } : {}),
        });
        return true;
      },
    },
    storage: {
      async getItem(key) {
        return storage[key];
      },
      async setItem(key, value) {
        storage[key] = value;
      },
      async removeItem(key) {
        delete storage[key];
      },
    },
    async confirmAlert() {
      return options.confirmDelete ?? true;
    },
    async showInFinder() {},
    async trash(path) {
      trashCalls.push(path);
    },
    async spawnBackend(spawnOptions): Promise<KunkunBackendConnection<SpaceLensAPI>> {
      const backendId = `backend-${++backendCounter}`;
      spawnedBackends.push(spawnOptions);
      return {
        backendId,
        api: backend,
        async destroy() {
          destroyedBackends.push(backendId);
        },
      };
    },
  };

  return {
    api: createKunkunClientWithAdapter(adapter),
    backend,
    spawnedBackends,
    destroyedBackends,
    permissionChecks,
    permissionRequests,
    trashCalls,
    pathCalls,
    storage,
  };
}

describe("createKunkunClientWithAdapter", () => {
  test("loads scan targets through Kunkun path APIs without spawning backend", async () => {
    const harness = createHarness();

    await expect(harness.api.getScanTargets()).resolves.toEqual([
      {
        id: "home",
        label: "Home",
        path: "/Users/tester",
        kind: "folder",
        description: "/Users/tester",
        size: 0,
        source: "preset",
      },
      {
        id: "desktop",
        label: "Desktop",
        path: "/Users/tester/Desktop",
        kind: "folder",
        description: "/Users/tester/Desktop",
        size: 0,
        source: "preset",
      },
      {
        id: "downloads",
        label: "Downloads",
        path: "/Users/tester/Downloads",
        kind: "folder",
        description: "/Users/tester/Downloads",
        size: 0,
        source: "preset",
      },
      {
        id: "documents",
        label: "Documents",
        path: "/Users/tester/Documents",
        kind: "folder",
        description: "/Users/tester/Documents",
        size: 0,
        source: "preset",
      },
    ]);

    expect(harness.pathCalls).toEqual([
      "homeDir",
      "desktopDir",
      "downloadDir",
      "documentDir",
    ]);
    expect(harness.spawnedBackends).toEqual([]);
  });

  test("loads recent scan targets before presets and dedupes preset paths", async () => {
    const harness = createHarness({
      storage: {
        "space-lens.recentScanTargets.v1": JSON.stringify([
          {
            path: "/Users/tester",
            label: "Tester Home",
            lastScannedAt: "2026-06-17T08:00:00.000Z",
            scanCount: 2,
          },
          {
            path: "/tmp/newer",
            label: "newer",
            lastScannedAt: "2026-06-17T09:00:00.000Z",
            scanCount: 1,
          },
        ]),
      },
    });

    await expect(harness.api.getScanTargets()).resolves.toEqual([
      {
        id: "recent:/tmp/newer",
        label: "newer",
        path: "/tmp/newer",
        kind: "folder",
        description: "/tmp/newer",
        size: 0,
        source: "recent",
        removable: true,
        lastScannedAt: "2026-06-17T09:00:00.000Z",
      },
      {
        id: "recent:/Users/tester",
        label: "Tester Home",
        path: "/Users/tester",
        kind: "folder",
        description: "/Users/tester",
        size: 0,
        source: "recent",
        removable: true,
        lastScannedAt: "2026-06-17T08:00:00.000Z",
      },
      {
        id: "desktop",
        label: "Desktop",
        path: "/Users/tester/Desktop",
        kind: "folder",
        description: "/Users/tester/Desktop",
        size: 0,
        source: "preset",
      },
      {
        id: "downloads",
        label: "Downloads",
        path: "/Users/tester/Downloads",
        kind: "folder",
        description: "/Users/tester/Downloads",
        size: 0,
        source: "preset",
      },
      {
        id: "documents",
        label: "Documents",
        path: "/Users/tester/Documents",
        kind: "folder",
        description: "/Users/tester/Documents",
        size: 0,
        source: "preset",
      },
    ]);
  });

  test("requests fs-read before scan and records recent targets", async () => {
    const harness = createHarness({ readAllowed: false });

    await expect(
      harness.api.startScan(startOptions(["/tmp/space-lens"])),
    ).resolves.toMatchObject({
      scanId: "scan-1",
    });

    expect(harness.permissionChecks).toContainEqual({
      permissionType: "fs-read",
      scopePattern: "/tmp/space-lens/**",
    });
    expect(harness.permissionRequests).toContainEqual({
      permissionType: "fs-read",
      scopePattern: "/tmp/space-lens/**",
      reason: "Scan this folder with Space Lens.",
    });
    expect(harness.spawnedBackends).toEqual([
      {
        scriptPath: "$EXTENSION/dist/backend.js",
        runtime: "deno",
      },
    ]);
    expect(harness.backend.scans[0]?.paths).toEqual(["/tmp/space-lens"]);
    expect(
      JSON.parse(harness.storage["space-lens.recentScanTargets.v1"] ?? "[]"),
    ).toMatchObject([
      {
        path: "/tmp/space-lens",
        label: "space-lens",
        scanCount: 1,
      },
    ]);
  });

  test("updates each successfully scanned path in recent target storage", async () => {
    const harness = createHarness({
      readAllowed: true,
      storage: {
        "space-lens.recentScanTargets.v1": JSON.stringify([
          {
            path: "/tmp/one",
            label: "one",
            lastScannedAt: "2026-06-17T08:00:00.000Z",
            scanCount: 3,
          },
        ]),
      },
    });

    await harness.api.startScan(startOptions(["/tmp/one/", "/tmp/two"]));

    const stored = JSON.parse(
      harness.storage["space-lens.recentScanTargets.v1"] ?? "[]",
    ) as Array<{ path: string; label: string; scanCount: number }>;
    expect(stored.map((entry) => entry.path)).toEqual(["/tmp/two", "/tmp/one"]);
    expect(stored.find((entry) => entry.path === "/tmp/one")?.scanCount).toBe(4);
    expect(stored.find((entry) => entry.path === "/tmp/two")?.label).toBe("two");
  });

  test("forgets only the requested recent scan target", async () => {
    const harness = createHarness({
      storage: {
        "space-lens.recentScanTargets.v1": JSON.stringify([
          {
            path: "/tmp/one",
            label: "one",
            lastScannedAt: "2026-06-17T08:00:00.000Z",
            scanCount: 1,
          },
          {
            path: "/tmp/two",
            label: "two",
            lastScannedAt: "2026-06-17T09:00:00.000Z",
            scanCount: 1,
          },
        ]),
      },
    });

    await harness.api.forgetScanTarget?.("/tmp/one/");

    const stored = JSON.parse(
      harness.storage["space-lens.recentScanTargets.v1"] ?? "[]",
    ) as Array<{ path: string }>;
    expect(stored.map((entry) => entry.path)).toEqual(["/tmp/two"]);
  });

  test("ignores invalid recent target storage without crashing", async () => {
    const harness = createHarness({
      storage: {
        "space-lens.recentScanTargets.v1": "not json",
      },
    });

    await expect(harness.api.getScanTargets()).resolves.toHaveLength(4);
  });

  test("respawns backend when a later scan needs a wider read set", async () => {
    const harness = createHarness({ readAllowed: true });

    await harness.api.startScan(startOptions(["/tmp/one"]));
    await harness.api.startScan(startOptions(["/tmp/one"]));
    await harness.api.startScan(startOptions(["/tmp/one", "/tmp/two"]));

    expect(harness.spawnedBackends).toHaveLength(2);
    expect(harness.destroyedBackends).toEqual(["backend-1"]);
  });

  test("cancels cleanup without host trash or backend native delete when user declines", async () => {
    const harness = createHarness({ confirmDelete: false });

    await expect(harness.api.executeCleanup(cleanupOptions())).resolves.toEqual(
      {
        removed: [],
        bytesRemoved: 0,
        errors: ["Deletion cancelled."],
      },
    );

    expect(harness.trashCalls).toEqual([]);
    expect(harness.backend.nativeDeleteCalls).toEqual([]);
  });

  test("requests fs-write and deletes through host trash, not backend native delete", async () => {
    const harness = createHarness({ writeAllowed: false });

    await expect(harness.api.executeCleanup(cleanupOptions())).resolves.toEqual(
      {
        removed: removalPlan().entries,
        bytesRemoved: 123,
        errors: [],
      },
    );

    expect(harness.permissionChecks).toContainEqual({
      permissionType: "fs-write",
      scopePattern: "/tmp/space-lens/old.log/**",
    });
    expect(harness.permissionRequests).toContainEqual({
      permissionType: "fs-write",
      scopePattern: "/tmp/space-lens/old.log/**",
      reason: "Move this Space Lens cleanup item to Trash.",
    });
    expect(harness.trashCalls).toEqual([["/tmp/space-lens/old.log"]]);
    expect(harness.backend.nativeDeleteCalls).toEqual([]);
  });

  test("uses original scan roots for cleanup planning without respawning the backend", async () => {
    const harness = createHarness({ readAllowed: true, writeAllowed: true });

    await harness.api.startScan(startOptions(["/tmp/space-lens"]));
    await expect(
      harness.api.executeCleanup(
        cleanupOptions("/tmp/space-lens/nested/old.log"),
      ),
    ).resolves.toEqual({
      removed: removalPlan().entries,
      bytesRemoved: 123,
      errors: [],
    });

    expect(harness.spawnedBackends).toHaveLength(1);
    expect(harness.destroyedBackends).toEqual([]);
    expect(harness.backend.cleanupPlans[0]?.entries[0]?.path).toBe(
      "/tmp/space-lens/nested/old.log",
    );
  });

  test("respawns with scan roots when cleanup follows a destroyed backend", async () => {
    const harness = createHarness({ readAllowed: true, writeAllowed: true });

    const firstScan = await harness.api.startScan(
      startOptions(["/tmp/space-lens"]),
    );
    await harness.api.startScan(startOptions(["/tmp/other"]));
    await expect(
      harness.api.planCleanup({
        ...cleanupOptions("/tmp/space-lens/old.log"),
        scanId: firstScan.scanId,
      }),
    ).resolves.toEqual(removalPlan());

    expect(harness.spawnedBackends).toHaveLength(3);
    expect(harness.destroyedBackends).toEqual(["backend-1", "backend-2"]);
  });
});
