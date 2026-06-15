import { webSocketClientTransport } from "kkrpc/ws";
import type { SpaceLensAPI } from "./types";
import { createTransportClient } from "./transport-client";

export function createRpcClient(url: string): SpaceLensAPI {
  return createTransportClient(webSocketClientTransport({ url }));
}
