import { wrap } from "kkrpc";
import type { RPCMessage, Transport } from "kkrpc";
import type { SpaceLensAPI } from "./types";

export function createTransportClient(
  transport: Transport<RPCMessage>,
): SpaceLensAPI {
  return wrap<SpaceLensAPI>(transport);
}
