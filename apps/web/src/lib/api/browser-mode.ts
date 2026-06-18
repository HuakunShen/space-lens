import { dev } from "$app/environment";
import { toWebSocketRpcUrl } from "./rpc-url";
import type { RuntimeMode } from "./types";

const DEV_RPC_URL = "ws://127.0.0.1:8757/rpc";

export function resolveRuntimeMode(
  location: Location | undefined = currentLocation(),
): RuntimeMode {
  const params = new URLSearchParams(location?.search ?? "");
  const requestedMode = params.get("spaceLensMode");
  if (
    requestedMode === "demo" ||
    requestedMode === "rpc" ||
    requestedMode === "http" ||
    requestedMode === "kunkun"
  ) {
    return requestedMode === "http" ? "rpc" : requestedMode;
  }
  if (hasKunkunIpc()) return "kunkun";
  if (isKunkunCustomViewRoute(location)) return "kunkun";
  if (params.has("spaceLensRpc") || params.has("spaceLensApi")) return "rpc";
  if (dev) return "rpc";
  if (location) return "rpc";
  return "demo";
}

export function resolveRpcUrl(
  location: Location | undefined = currentLocation(),
): string {
  const params = new URLSearchParams(location?.search ?? "");
  const raw = params.get("spaceLensRpc") ?? params.get("spaceLensApi");
  if (raw && raw !== "1" && raw !== "true" && raw !== "auto") {
    return toWebSocketRpcUrl(raw, location);
  }
  if (!raw && dev) return DEV_RPC_URL;
  const origin = location?.origin ?? "http://127.0.0.1:8757";
  return toWebSocketRpcUrl(origin, location);
}

function currentLocation(): Location | undefined {
  return globalThis.window?.location;
}

function hasKunkunIpc(): boolean {
  return Boolean(
    (globalThis as { window?: { electron?: { ipcRenderer?: unknown } } }).window
      ?.electron?.ipcRenderer,
  );
}

function isKunkunCustomViewRoute(location: Location | undefined): boolean {
  return Boolean(location?.pathname?.startsWith("/custom-views/"));
}
