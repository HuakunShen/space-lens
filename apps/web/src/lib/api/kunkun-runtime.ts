/**
 * Runtime boundary used by Space Lens' shared web app for Kunkun mode.
 *
 * The web UI owns SpaceLensAPI behavior, while the Kunkun plugin package owns
 * host bootstrap and kkrpc transport wiring through @kunkunsh/sdk/ui/custom.
 */
import type { SpaceLensAPI } from "./types.js";

export type KunkunBackendRuntime = "auto" | "node" | "bun" | "deno";

export interface KunkunBackendConnection<RemoteAPI> {
  readonly api: RemoteAPI;
  readonly backendId?: string;
  destroy(): Promise<void>;
}

export interface KunkunRuntimeAdapter {
  readonly path: {
    homeDir(): Promise<string>;
    desktopDir(): Promise<string>;
    downloadDir(): Promise<string>;
    documentDir(): Promise<string>;
  };
  readonly permissions: {
    check(permissionType: string, scopePattern: string): Promise<boolean>;
    request(
      permissionType: string,
      scopePattern: string,
      reason?: string,
    ): Promise<boolean>;
  };
  readonly storage: {
    getItem(key: string): Promise<string | undefined>;
    setItem(key: string, value: string): Promise<void>;
    removeItem(key: string): Promise<void>;
  };
  confirmAlert(options: {
    title: string;
    message?: string;
    primaryAction?: { title: string };
  }): Promise<boolean>;
  showInFinder(path: string): Promise<void>;
  trash(path: string | string[]): Promise<void>;
  spawnBackend(options: {
    scriptPath: string;
    runtime?: KunkunBackendRuntime;
  }): Promise<KunkunBackendConnection<SpaceLensAPI>>;
}
