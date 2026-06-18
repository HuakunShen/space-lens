/**
 * Electron custom-view transport smoke for the Space Lens Kunkun adapter.
 * This uses in-memory Electron-like endpoints to prove createKunkunClient()
 * resolves Kunkun host IPC and backend relay channels through kkrpc.
 */
import { afterEach, describe, expect, test } from "bun:test";
import { RPCChannel } from "kkrpc/streaming";
import { electronIpcTransport, type ElectronMessageEndpoint } from "kkrpc/electron";
import type { RPCMessage } from "kkrpc";
import {
  createKunkunClient,
  resetKunkunClientForTests,
  type KunkunHostAPI,
} from "../src/lib/api/kunkun-client";
import type {
  CleanupPlanOptions,
  ExecuteCleanupOptions,
  GetChildrenRequest,
  GetNodeRequest,
  RemovalPlan,
  SpaceLensAPI,
  StartScanOptions,
} from "../src/lib/api/types";

const originalWindowDescriptor = Object.getOwnPropertyDescriptor(globalThis, "window");

afterEach(() => {
  resetKunkunClientForTests();
  if (originalWindowDescriptor) {
    Object.defineProperty(globalThis, "window", originalWindowDescriptor);
  } else {
    Reflect.deleteProperty(globalThis, "window");
  }
});

describe("createKunkunClient Electron custom-view transport", () => {
  test("scans through backend relay and deletes through host trash", async () => {
    const endpoints = createElectronEndpointPair();
    const backend = createFakeBackend();
    const hostEvents = {
      permissionChecks: [] as Array<{ permissionType: string; scopePattern: string }>,
      permissionRequests: [] as Array<{ permissionType: string; scopePattern: string; reason?: string }>,
      spawnOptions: [] as Array<{ scriptPath: string; runtime?: string; fsReadAllow?: readonly string[] }>,
      trashCalls: [] as Array<string | string[]>,
      confirmations: 0,
    };
    const channels: Array<{ destroy(): void }> = [];

    const host: KunkunHostAPI = {
      backend: {
        async spawn(scriptPath, runtime, options) {
          hostEvents.spawnOptions.push({
            scriptPath,
            runtime,
            fsReadAllow: options?.fsReadAllow,
          });
          channels.push(new RPCChannel<SpaceLensAPI, object>(
            electronIpcTransport({ endpoint: endpoints.main, channel: "backend-space-lens-1" }),
            { expose: backend },
          ));
          return { backendId: "backend-1", channel: "backend-space-lens-1" };
        },
        async kill() {},
      },
      path: {
        async homeDir() {
          return "/Users/tester";
        },
        async desktopDir() {
          return "/Users/tester/Desktop";
        },
        async downloadDir() {
          return "/Users/tester/Downloads";
        },
        async documentDir() {
          return "/Users/tester/Documents";
        },
      },
      permissions: {
        async check(permissionType, scopePattern) {
          hostEvents.permissionChecks.push({ permissionType, scopePattern });
          return permissionType === "fs-read";
        },
        async request(permissionType, scopePattern, reason) {
          hostEvents.permissionRequests.push({ permissionType, scopePattern, ...(reason ? { reason } : {}) });
          return true;
        },
      },
      system: {
        async showInFinder() {},
        async trash(path) {
          hostEvents.trashCalls.push(path);
        },
      },
      ui: {
        async confirmAlert() {
          hostEvents.confirmations += 1;
          return true;
        },
      },
    };

    channels.push(new RPCChannel<KunkunHostAPI, object>(
      electronIpcTransport({ endpoint: endpoints.main, channel: "kkrpc-plugin-com.space-lens.app" }),
      { expose: host },
    ));

    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: {
        electron: { ipcRenderer: endpoints.renderer },
        location: new URL("http://space-lens.test/?pluginId=com.space-lens.app"),
      },
    });

    const client = createKunkunClient();
    await expect(client.getScanTargets()).resolves.toEqual([
      {
        id: "home",
        label: "Home",
        path: "/Users/tester",
        kind: "folder",
        description: "/Users/tester",
        size: 0,
      },
      {
        id: "desktop",
        label: "Desktop",
        path: "/Users/tester/Desktop",
        kind: "folder",
        description: "/Users/tester/Desktop",
        size: 0,
      },
      {
        id: "downloads",
        label: "Downloads",
        path: "/Users/tester/Downloads",
        kind: "folder",
        description: "/Users/tester/Downloads",
        size: 0,
      },
      {
        id: "documents",
        label: "Documents",
        path: "/Users/tester/Documents",
        kind: "folder",
        description: "/Users/tester/Documents",
        size: 0,
      },
    ]);

    await expect(client.startScan(startOptions(["/Users/tester/Downloads"]))).resolves.toMatchObject({
      scanId: "scan-1",
      rootIds: ["root"],
    });
    await expect(client.executeCleanup(cleanupOptions())).resolves.toEqual({
      removed: removalPlan().entries,
      bytesRemoved: 2048,
      errors: [],
    });

    expect(hostEvents.spawnOptions).toEqual([{
      scriptPath: "$EXTENSION/dist/backend.js",
      runtime: "node",
      fsReadAllow: ["/Users/tester/Downloads"],
    }]);
    expect(hostEvents.permissionChecks).toContainEqual({
      permissionType: "fs-read",
      scopePattern: "/Users/tester/Downloads/**",
    });
    expect(hostEvents.permissionRequests).toContainEqual({
      permissionType: "fs-write",
      scopePattern: "/Users/tester/Downloads/old.log/**",
      reason: "Move this Space Lens cleanup item to Trash.",
    });
    expect(hostEvents.confirmations).toBe(1);
    expect(hostEvents.trashCalls).toEqual([["/Users/tester/Downloads/old.log"]]);
    expect(backend.nativeDeletes).toEqual([]);

    for (const channel of channels) channel.destroy();
  });
});

class FakeElectronEndpoint implements ElectronMessageEndpoint {
  peer?: FakeElectronEndpoint;
  private readonly listeners = new Map<string, Set<(_event: unknown, message: RPCMessage) => void>>();

  send(channel: string, message: RPCMessage): void {
    queueMicrotask(() => this.peer?.emit(channel, message));
  }

  on(channel: string, listener: (_event: unknown, message: RPCMessage) => void): void {
    const listeners = this.listeners.get(channel) ?? new Set();
    listeners.add(listener);
    this.listeners.set(channel, listeners);
  }

  off(channel: string, listener: (_event: unknown, message: RPCMessage) => void): void {
    this.listeners.get(channel)?.delete(listener);
  }

  private emit(channel: string, message: RPCMessage): void {
    for (const listener of this.listeners.get(channel) ?? []) {
      listener({}, message);
    }
  }
}

function createElectronEndpointPair(): {
  renderer: FakeElectronEndpoint;
  main: FakeElectronEndpoint;
} {
  const renderer = new FakeElectronEndpoint();
  const main = new FakeElectronEndpoint();
  renderer.peer = main;
  main.peer = renderer;
  return { renderer, main };
}

function startOptions(paths: string[]): StartScanOptions {
  return {
    paths,
    ignoreHidden: false,
    respectGitignore: true,
    ignoredMode: "summarize",
    initialDepth: 1,
    maxChildrenPerNode: 10,
  };
}

function cleanupOptions(): ExecuteCleanupOptions {
  return {
    scanId: "scan-1",
    entries: [{
      id: "scan-1:old-log",
      scanId: "scan-1",
      nodeId: "old-log",
      path: "/Users/tester/Downloads/old.log",
      name: "old.log",
      size: 2048,
      addedAt: "2026-06-17T00:00:00.000Z",
    }],
  };
}

function removalPlan(): RemovalPlan {
  return {
    entries: [{
      path: "/Users/tester/Downloads/old.log",
      size: 2048,
      reason: "manual",
      preset: "collector",
    }],
    totalSize: 2048,
    errors: [],
  };
}

function createFakeBackend(): SpaceLensAPI & {
  nativeDeletes: ExecuteCleanupOptions[];
} {
  const nativeDeletes: ExecuteCleanupOptions[] = [];
  return {
    nativeDeletes,
    async getScanTargets() {
      return [];
    },
    async startScan(options: StartScanOptions) {
      return {
        scanId: "scan-1",
        rootIds: ["root"],
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
        bytesScanned: 2048,
        entriesScanned: 1,
        rootIds: ["root"],
        label: "Downloads",
        updatedAt: "2026-06-17T00:00:00.000Z",
      };
    },
    async cancelScan() {},
    async planCleanup(_options: CleanupPlanOptions) {
      return removalPlan();
    },
    async executeCleanup(options) {
      nativeDeletes.push(options);
      return {
        removed: removalPlan().entries,
        bytesRemoved: removalPlan().totalSize,
        errors: [],
      };
    },
  };
}
