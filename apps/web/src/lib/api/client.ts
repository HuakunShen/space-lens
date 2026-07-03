import { resolveRpcUrl, resolveRuntimeMode } from "./browser-mode";
import type { RuntimeMode, SpaceLensAPI } from "./types";

export interface SpaceLensClient {
  mode: RuntimeMode;
  api: SpaceLensAPI;
}

export async function createSpaceLensClient(): Promise<SpaceLensClient> {
  const mode = resolveRuntimeMode();
  if (mode === "rpc") {
    const { createRpcClient } = await import("./rpc-client");
    return { mode, api: createRpcClient(resolveRpcUrl()) };
  }
  if (mode === "kunkun") {
    const { createKunkunClient } = await import("./kunkun-client");
    return { mode, api: createKunkunClient() };
  }
  const { createDemoClient } = await import("./demo-client");
  return { mode: "demo", api: createDemoClient() };
}
