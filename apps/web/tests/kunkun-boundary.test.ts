/**
 * Boundary tests for the Space Lens Kunkun web adapter.
 *
 * Kunkun custom-view transport setup belongs in @kunkunsh/sdk/ui/custom, not in
 * Space Lens' shared Svelte web app. Standalone browser RPC remains separate.
 */
import { describe, expect, test } from "bun:test";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

type PackageJson = {
  dependencies?: Record<string, string>;
  devDependencies?: Record<string, string>;
  peerDependencies?: Record<string, string>;
  peerDependenciesMeta?: Record<string, { optional?: boolean }>;
};

describe("Space Lens Kunkun web boundary", () => {
  test("does not declare unpublished Kunkun SDK in the web package manifest", async () => {
    const packageJson = JSON.parse(
      await readFile(resolve(import.meta.dir, "../package.json"), "utf8"),
    ) as PackageJson;

    expect(packageJson.dependencies?.["@kunkunsh/sdk"]).toBeUndefined();
    expect(packageJson.devDependencies?.["@kunkunsh/sdk"]).toBeUndefined();
    expect(packageJson.peerDependencies?.["@kunkunsh/sdk"]).toBeUndefined();
    expect(packageJson.peerDependenciesMeta?.["@kunkunsh/sdk"]).toBeUndefined();
  });

  test("client factory dynamically imports runtime clients", async () => {
    const source = await readFile(
      resolve(import.meta.dir, "../src/lib/api/client.ts"),
      "utf8",
    );

    expect(source).toContain('await import("./rpc-client")');
    expect(source).toContain('await import("./kunkun-client")');
    expect(source).toContain('await import("./demo-client")');
    expect(source).not.toContain('import { createKunkunClient }');
    expect(source).not.toContain('import { createRpcClient }');
    expect(source).not.toContain('import { createDemoClient }');
  });

  test("vite config aliases only the Kunkun SDK loader by build mode", async () => {
    const source = await readFile(
      resolve(import.meta.dir, "../vite.config.ts"),
      "utf8",
    );

    expect(source).toContain("SPACE_LENS_KUNKUN_BUILD");
    expect(source).toContain("#space-lens/kunkun-sdk-loader");
    expect(source).toContain("kunkun-sdk-loader.kunkun.ts");
    expect(source).not.toContain("#space-lens/kunkun-runtime");
  });

  test("Kunkun SDK loader uses literal imports for Vite bundling", async () => {
    const source = await readFile(
      resolve(import.meta.dir, "../src/lib/api/kunkun-sdk-loader.kunkun.ts"),
      "utf8",
    );

    expect(source).toContain('import("@kunkunsh/sdk/ui/custom")');
    expect(source).toContain('import("@kunkunsh/sdk")');
    expect(source).not.toContain("import(kunkun");
  });

  test("kunkun-client does not construct Kunkun host kkrpc transports", async () => {
    const source = await readFile(
      resolve(import.meta.dir, "../src/lib/api/kunkun-client.ts"),
      "utf8",
    );

    expect(source).not.toContain("RPCChannel");
    expect(source).not.toContain("electronIpcTransport");
    expect(source).not.toContain("webSocketClientTransport");
    expect(source).not.toContain("KunkunHostAPI");
    expect(source).not.toContain("KunkunPluginAPI");
    expect(source).not.toContain("resolveKunkunHostConnection");
    expect(source).not.toContain("kkrpc/electron");
    expect(source).not.toContain("kkrpc/ws");
    expect(source).not.toContain("#space-lens/kunkun-runtime");
  });

  test("runtime mode detection does not use raw Electron IPC presence", async () => {
    const source = await readFile(
      resolve(import.meta.dir, "../src/lib/api/browser-mode.ts"),
      "utf8",
    );

    expect(source).not.toContain("ipcRenderer");
    expect(source).not.toContain("window?.electron");
    expect(source).toContain("__kunkun__");
    expect(source).toContain("/custom-views/");
  });
});
