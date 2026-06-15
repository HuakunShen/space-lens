import { dev } from "$app/environment";
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
  if (raw && raw !== "1" && raw !== "true" && raw !== "auto")
    return toWebSocketRpcUrl(raw);
  if (!raw && dev) return DEV_RPC_URL;
  const origin = location?.origin ?? "http://127.0.0.1:8757";
  return toWebSocketRpcUrl(origin);
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

function toWebSocketRpcUrl(raw: string): string {
  let trimmed = raw.replace(/\/$/, "");
  if (trimmed.startsWith("ws://") || trimmed.startsWith("wss://")) {
    return normalizeRpcPath(new URL(trimmed)).toString();
  }
  if (trimmed.startsWith("/")) {
    const origin = currentLocation()?.origin ?? "http://127.0.0.1:8757";
    trimmed = `${origin}${trimmed}`;
  }
  if (!/^[a-z]+:\/\//i.test(trimmed)) {
    trimmed = `http://${trimmed}`;
  }
  const url = new URL(trimmed);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return normalizeRpcPath(url).toString();
}

function normalizeRpcPath(url: URL): URL {
  if (url.pathname === "/api" || url.pathname.endsWith("/api")) {
    url.pathname = `${url.pathname.slice(0, -"/api".length)}/rpc`;
  } else if (!url.pathname.endsWith("/rpc")) {
    url.pathname = `${url.pathname.replace(/\/$/, "")}/rpc`;
  }
  url.search = "";
  url.hash = "";
  return url;
}
