/**
 * Tests for standalone RPC transport behavior layered on top of SpaceLensAPI.
 * The Svelte page should not own destructive-operation confirmation; standalone
 * browser mode confirms before calling the privileged WebSocket API.
 */
import { describe, expect, test } from "bun:test";
import { withCleanupConfirmation } from "../src/lib/api/rpc-client";
import type {
  CleanupPlanOptions,
  ExecuteCleanupOptions,
  GetChildrenRequest,
  GetNodeRequest,
  RemovalPlan,
  SpaceLensAPI,
  StartScanOptions,
} from "../src/lib/api/types";

function cleanupOptions(): ExecuteCleanupOptions {
  return {
    scanId: "scan-1",
    entries: [{
      id: "entry-1",
      scanId: "scan-1",
      nodeId: "node-1",
      path: "/tmp/space-lens/old.log",
      name: "old.log",
      size: 2048,
      addedAt: "2026-06-17T00:00:00.000Z",
    }],
  };
}

function removalPlan(): RemovalPlan {
  return {
    entries: [{
      path: "/tmp/space-lens/old.log",
      size: 2048,
      reason: "manual",
      preset: "collector",
    }],
    totalSize: 2048,
    errors: [],
  };
}

function fakeApi(plan: RemovalPlan = removalPlan()): SpaceLensAPI & {
  readonly deleteCalls: ExecuteCleanupOptions[];
  readonly planCalls: CleanupPlanOptions[];
} {
  const deleteCalls: ExecuteCleanupOptions[] = [];
  const planCalls: CleanupPlanOptions[] = [];
  return {
    deleteCalls,
    planCalls,
    async getScanTargets() {
      return [];
    },
    async startScan(_options: StartScanOptions) {
      throw new Error("not used");
    },
    async getNode(_request: GetNodeRequest) {
      throw new Error("not used");
    },
    async getChildren(_request: GetChildrenRequest) {
      throw new Error("not used");
    },
    async getScanStatus(scanId) {
      throw new Error(`not used ${scanId}`);
    },
    async cancelScan() {},
    async planCleanup(options) {
      planCalls.push(options);
      return plan;
    },
    async executeCleanup(options) {
      deleteCalls.push(options);
      return { removed: plan.entries, bytesRemoved: plan.totalSize, errors: plan.errors };
    },
  };
}

describe("withCleanupConfirmation", () => {
  test("does not call privileged cleanup when the user cancels", async () => {
    const api = fakeApi();
    const client = withCleanupConfirmation(api, () => false);

    await expect(client.executeCleanup(cleanupOptions())).resolves.toEqual({
      removed: [],
      bytesRemoved: 0,
      errors: ["Deletion cancelled."],
    });

    expect(api.planCalls.length).toBe(1);
    expect(api.deleteCalls).toEqual([]);
  });

  test("calls privileged cleanup only after confirmation", async () => {
    const api = fakeApi();
    const messages: string[] = [];
    const client = withCleanupConfirmation(api, (message) => {
      messages.push(message);
      return true;
    });

    await expect(client.executeCleanup(cleanupOptions())).resolves.toEqual({
      removed: removalPlan().entries,
      bytesRemoved: 2048,
      errors: [],
    });

    expect(messages).toEqual(["Delete 1 selected items (2.0 KiB)?"]);
    expect(api.deleteCalls).toEqual([cleanupOptions()]);
  });
});
