# Technology Stack

**Updated: 2026-09-23** — covers the multi-runtime workbench line (commits
1e2c056, e96ec62, 9097fa9, 7dd76ca, 30990e3) as of `5d74b29`.

## Workspaces

Yarn 4.14.1 workspaces (root `package.json`): `packages/contract`,
`packages/client`, `packages/host`, `packages/node`, `packages/web-ui`,
`apps/tui`, `apps/vscode`, `apps/desktop`, `apps/web`. Rust side is a
Cargo workspace; `apps/desktop/src-tauri` is its **own** Cargo workspace.

## Web workbench (one SPA, two flavors)

- `apps/web` — SvelteKit SPA + PWA, Cloudflare deploy (wrangler dry-run in
  CI). Flavor switch lives in `apps/web/svelte.config.js`; the desktop flavor
  is compiled into the Tauri shell (`scripts/build-desktop.ts` orchestrator:
  web flavor first, then cargo).
- `packages/host` — Hono HTTP host: authenticated REST + SSE over the closed
  contract, static SPA serving, worker-thread scan sessions (`scan-store.ts`),
  pairing tickets → bearer sessions, CIDR/origin gates, trash-only cleanup.
- `packages/contract` — closed JSON contract, zod-validated, byte-fresh
  artifacts checked in CI.
- `packages/client` — transport adapter: browser HTTP/SSE **and** Tauri
  Channel replies (used by `apps/web` and `apps/vscode`).
- `packages/web-ui` — shared Svelte components (SunburstChart, ChildList,
  StatusBar, ScanPicker, theme).

## Tauri desktop shell

- macOS `invoke` over the custom protocol lost responses after the third
  call; **resolved**: every `sl_*` command replies through a Tauri
  **Channel** (event delivery) and returns `Ok(())` in the fetch body
  (`packages/client/src/tauri.ts`).
- IPC is a closed command set (`sl_*`); requests are tagged unions with
  `deny_unknown_fields` — an unknown method is a typed error, never a
  deserialization crash.
- No JS runtime ships: Svelte bundle is compiled in; engine is `kuntu-scan`
  linked directly. Cleanup is trash-only (`trash` crate), re-verified at
  execution.
- Updater: minisign key in `tauri.conf.json`; private key only in
  `TAURI_SIGNING_PRIVATE_KEY*` secrets.

## VS Code extension

`apps/vscode` — supervisor (`supervisor.ts`) spawns
`spacelens serve --machine --port 0 --json --no-open` (readiness JSON on
stdout, single-use pairing ticket on stderr, never exposed to the webview);
webview renders `@space-lens/web-ui` through `@space-lens/client`.

## Native macOS

- `apps/space-lens-mac` — SwiftUI; one Canvas sunburst with cached segment
  layout; Phase-0 synthetic demo + read-only real scan through the C ABI FFI;
  Collector review → macOS Trash; Apple-only iCloud Local Copies session.
- `apps/icloud-free` — SwiftUI GUI (macOS 26 Liquid Glass with translucent
  material fallback) + Swift Argument Parser CLI; bounded concurrent eviction
  (8 → cap 32) with pause/resume/stop.

## Language runtimes

Rust (engine, CLI, Tauri commands, napi-rs 3), TypeScript (contract/host/client/
web-ui/vscode), Svelte 5, Swift (two macOS apps), Solid/Uniview (TUI).
