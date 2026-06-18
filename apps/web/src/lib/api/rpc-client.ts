import { webSocketClientTransport } from "kkrpc/ws";
import type { CleanupOutcome, ExecuteCleanupOptions, SpaceLensAPI } from "./types";
import { createTransportClient } from "./transport-client";

export function createRpcClient(url: string): SpaceLensAPI {
  return withCleanupConfirmation(createTransportClient(webSocketClientTransport({ url })));
}

export function withCleanupConfirmation(
  api: SpaceLensAPI,
  confirm: (message: string) => boolean | undefined = (message) => globalThis.window?.confirm(message),
): SpaceLensAPI {
  return {
    ...api,
    async executeCleanup(options: ExecuteCleanupOptions): Promise<CleanupOutcome> {
      const plan = await api.planCleanup(options);
      if (plan.entries.length === 0) {
        return { removed: [], bytesRemoved: 0, errors: plan.errors };
      }
      const confirmed = confirm(`Delete ${plan.entries.length} selected items (${formatBytes(plan.totalSize)})?`);
      if (!confirmed) {
        return {
          removed: [],
          bytesRemoved: 0,
          errors: ["Deletion cancelled."],
        };
      }
      return api.executeCleanup(options);
    },
  };
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** exponent;
  return `${value >= 10 || exponent === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[exponent]}`;
}
