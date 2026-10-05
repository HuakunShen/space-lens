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

## Native interface styles

**Updated: 2026-10-05** — shared shadcn-svelte primitives and product components
use `macos:`, `windows:` and `linux:` Tailwind variants selected by the root
`data-interface` attribute. `styles/platform.css` contains semantic colors,
fonts and materials; component utilities own control dimensions and layout.
Base theme/layout CSS is layered below utilities so native treatments can
actually override the baseline. Keep future platform rules in the component.

- macOS: compact unified toolbar, source list, horizontal picker labels and
  file/folder symbols in compact directory rows.
- Windows: navigation selection bars, separate page heading/command actions,
  inset content surfaces and Fluent input bottom strokes.
- Linux: GNOME/libadwaita-inspired centered header title, larger raised controls,
  solid sidebar and grouped settings rows.
- Settings persist `auto | web | macos | windows | linux` with independent
  compact/comfortable density and system/light/dark appearance. Automatic
  chooses a native style in Tauri and Web in browsers; Android is not GNOME.
- Startup and workspace share `ScanSidebar` and `LensToolbar`; WinUI commands
  live in `WorkspaceHeading`. Native OS caption controls remain host-owned.
- Native graph sizing becomes absolute only at `md` and above; two-column
  utilities apply only while the map is visible. Both rules preserve narrow
  windows and the full-width contents-only view.

See `.journal/2026-10-05-1940.md` for rationale and verification scope.

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
