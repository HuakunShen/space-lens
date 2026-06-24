/**
 * Boundary tests for the Space Lens Kunkun web adapter.
 *
 * Kunkun custom-view transport setup belongs in @kunkunsh/api/ui/custom, not in
 * Space Lens' shared Svelte web app. Standalone browser RPC remains separate.
 */
import { describe, expect, test } from "bun:test";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

describe("Space Lens Kunkun web boundary", () => {
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

  test("vite config does not use build-target runtime aliasing", async () => {
    const source = await readFile(
      resolve(import.meta.dir, "../vite.config.ts"),
      "utf8",
    );

    expect(source).not.toContain("SPACE_LENS_BUILD_TARGET");
    expect(source).not.toContain("#space-lens/kunkun-runtime");
    expect(source).not.toContain("kunkunRuntimeAdapter");
    expect(source).not.toContain("kunkun-runtime.stub");
    expect(source).not.toContain("kunkun-runtime.adapter");
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
