# System Overview

**Updated: 2026-09-23** — initial wiki build from git history at `5d74b29` (no `.journal/` or `.reports/` entries existed at sync time).

Space Lens is a fast directory-scanning and cleanup-candidate toolkit. The Rust
engine (scanner, snapshot, cleanup, iCloud eviction) moved to
[Kuntu](https://github.com/HuakunShen/kuntu) as `kuntu-scan` (tag `v0.3.0`,
pinned at `vendors/kuntu`, commit 02869e7, 2026-09-14). The `space_lens::`
Rust API is unchanged — the dependency is aliased. This repository holds the
product shells around that engine.

## Product lines

1. **npm + TUI** — the `space-lens` npm package (napi-rs bindings) and the
   Solid/Uniview TUI (`@space-lens/cli`). See [NPM Package](NAPI/NPM%20Package.md)
   and [CLI and TUI](CLI/CLI%20and%20TUI.md).
2. **Serve / Web / Desktop / VS Code** — `spacelens serve` hosts the web UI
   behind a closed JSON contract (`@space-lens/contract`): single-use pairing
   tickets, bearer sessions, REST reads, SSE scan events. One SvelteKit SPA has
   browser and desktop flavors; the desktop flavor is embedded in a Tauri shell;
   the VS Code extension supervises `spacelens serve --machine` and renders a
   webview. See [Technology Stack](Technology/Technology%20Stack.md).
3. **Native macOS** — `apps/space-lens-mac` (SwiftUI, Canvas sunburst, C ABI
   FFI) and `apps/icloud-free` (SwiftUI + Swift CLI; Foundation
   `evictUbiquitousItem`).
4. **Rust CLI + MCP** — `apps/cli` with `scan`, `candidates`, `clean`,
   `icloud`, and a feature-gated `mcp` subcommand. See
   [CLI and TUI](CLI/CLI%20and%20TUI.md).

## Safety model

- Cleanup APIs are dry-run oriented; `clean` needs `--execute`.
- Web and desktop cleanup is trash-only; permanent deletion stays a CLI/TUI concern.
- The npm package plans cleanup but never deletes.
- iCloud eviction never falls back to deletion; execution needs `--execute`
  plus an interactive confirmation phrase. Bounded worker pool: 8 by default, capped at 32.

## Engineering

- Tooling: Yarn 4.14.1 workspaces + Cargo workspace; `just` recipes for the
  Swift apps. See [Technology Stack](Technology/Technology%20Stack.md).
- CI/release lines: `v*` (npm), `cli-v*` (@space-lens/cli), `app-v*`
  (desktop + vsix). See [CI and Release](CI/CI%20and%20Release.md).
- Engine internals: [Core Engine](Core/Core%20Engine.md).
