/**
 * Browser-safe URL normalization for Space Lens kkrpc WebSocket endpoints.
 * The standalone CLI prints an app URL containing an authenticated RPC URL, so
 * this helper must preserve query tokens while normalizing `/api` aliases to `/rpc`.
 */
export interface LocationLike {
  readonly origin: string;
}

export function toWebSocketRpcUrl(
  raw: string,
  location: LocationLike | undefined = currentLocation(),
): string {
  let trimmed = raw.replace(/\/$/, "");
  if (trimmed.startsWith("ws://") || trimmed.startsWith("wss://")) {
    return normalizeRpcPath(new URL(trimmed)).toString();
  }
  if (trimmed.startsWith("/")) {
    const origin = location?.origin ?? "http://127.0.0.1:8757";
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
  url.hash = "";
  return url;
}

function currentLocation(): LocationLike | undefined {
  return globalThis.window?.location;
}
