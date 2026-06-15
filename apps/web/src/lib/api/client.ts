import { resolveRpcUrl, resolveRuntimeMode } from "./browser-mode";
import { createDemoClient } from "./demo-client";
import { createKunkunClient } from "./kunkun-client";
import { createRpcClient } from "./rpc-client";
import type { RuntimeMode, SpaceLensAPI } from "./types";

export interface SpaceLensClient {
  mode: RuntimeMode;
  api: SpaceLensAPI;
}

export function createSpaceLensClient(): SpaceLensClient {
  const mode = resolveRuntimeMode();
  if (mode === "rpc") {
    return { mode, api: createRpcClient(resolveRpcUrl()) };
  }
  if (mode === "kunkun") {
    return { mode, api: createKunkunClient() };
  }
  return { mode: "demo", api: createDemoClient() };
}
