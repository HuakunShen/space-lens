# Space Lens

Fast directory scanning and cleanup candidate utilities, powered by Rust and napi-rs.

> **Where the engine went (2026-09-14):** the Rust engine that lived in
> `packages/space-lens` — scanner, snapshot, cleanup, iCloud eviction — moved to
> [Kuntu](https://github.com/HuakunShen/kuntu) as `kuntu-scan` (tag `v0.3.0`,
> pinned here at `vendors/kuntu`). This repo is the product shells: the
> SwiftUI apps (via `space-lens-ffi`), the npm package, the CLI/TUI, and the
> standalone iCloud app. The `space_lens::` Rust API is unchanged — the
> dependency is aliased.

## Installation

```bash
npm install space-lens
```

## Install the CLI

The Rust CLI is the crate `spacelens` (scan, candidates, clean, dirty-git,
icloud). Once published to crates.io:

```bash
cargo install spacelens        # build from source
cargo binstall spacelens       # or: prebuilt binary from the GitHub release
brew install HuakunShen/homebrew-tap/spacelens
```

Until then, install straight from this repository (cargo checks out the
vendors/kuntu submodule itself):

```bash
cargo install --git https://github.com/HuakunShen/space-lens --locked spacelens
```

## Releasing

Version bumping, publishing (crates.io / npm / Homebrew), tags, and the
packaging gotchas are documented step by step in [`docs/release.md`](docs/release.md).

## Serve the web workbench

`spacelens serve` starts an authenticated HTTP host that serves the web UI and
speaks a closed JSON contract (`@space-lens/contract`): single-use pairing
tickets, bearer sessions, REST reads, and SSE scan events. Cleanup over the
web is trash-only; permanent deletion stays a CLI/TUI concern.

```bash
npx spacelens serve                    # loopback, read-only, current directory
npx spacelens serve --host 0.0.0.0 --allow-cidr 192.168.1.0/24
npx spacelens serve --allow-cleanup    # grant trash-based cleanup
```

## API Usage

```ts
import { scanDirectory } from 'space-lens'

const tree = scanDirectory({
  directories: [process.cwd()],
  ignoreHidden: false,
  fullPath: false,
  respectGitignore: true,
  ignoredMode: 'summarize',
})

console.dir(tree, { depth: 3 })
```

Find cleanup candidates without deleting anything:

```ts
import { findCleanupCandidates, planCleanup } from 'space-lens'

const candidates = findCleanupCandidates({
  directories: [process.cwd()],
  presets: ['node', 'rust', 'gitignored'],
})

const plan = planCleanup({
  directories: [process.cwd()],
  presets: ['node'],
})
```

## Directory Scanning

`scanDirectory` is intended for large folders where keeping every file node in memory is too expensive.
With `ignoredMode: 'summarize'`, ignored directories such as `target/` or `node_modules/` are scanned for total size but returned as collapsed leaf nodes:

```ts
{
  name: 'target',
  path: '/path/to/project/target',
  size: 1238249472,
  children: [],
  ignored: true,
  collapsed: true
}
```

Use `ignoredMode: 'exclude'` to skip ignored paths entirely.

## Cleanup Candidates

Cleanup APIs are dry-run oriented. `findCleanupCandidates` reports matching paths and sizes, and `planCleanup` returns a removal plan. The npm package does not execute deletion.

Initial presets:

- `node`: reports `node_modules`.
- `rust`: reports Cargo `target` directories.
- `gitignored`: reports paths matched by `.gitignore`.

## Rust CLI

The workspace includes a simple Rust CLI app:

```bash
cargo run -p spacelens -- scan ~/Dev --json
cargo run -p spacelens -- candidates ~/Dev --preset node
cargo run -p spacelens -- clean ~/Dev --preset node
```

`clean` defaults to dry-run. Add `--execute` only when you want to remove the planned paths.

### iCloud local-copy MVP

The Rust core and CLI also include a macOS-only iCloud inspection/eviction flow. It is separate from the existing deletion cleanup code:

```bash
cargo run --release -p spacelens -- icloud --help
cargo run --release -p spacelens -- icloud inspect <absolute-path>
cargo run --release -p spacelens -- icloud plan <disposable-iCloud-test-folder>
cargo run --release -p spacelens -- icloud evict <disposable-iCloud-test-folder>
```

`evict` is dry-run by default. Real execution requires `--execute` and the interactive confirmation phrase. The implementation never falls back to deleting files. Do not use a real Lightroom or Photos folder as the first test target.

Rust eviction uses a bounded worker pool (8 concurrent items by default, capped
at 32), so a large plan does not create one task per file or process entries
serially. The same bounded execution path is used by the Rust CLI service.

The NAPI package exposes the same native boundary through `ICloudSession`, with opaque one-shot plan IDs and decimal-string byte counts. The current `main` TUI does not yet expose these iCloud actions; it still provides the existing scan/cleanup UI.

The published `space-lens` package supports both the native library and the TUI executable:

```ts
import { scanDirectory } from 'space-lens'
```

```bash
npx space-lens ~/Dev --preset rust
```

## space-lens TUI CLI

The `space-lens` package remains importable as a native library and also exposes the Solid/Uniview TUI as a `space-lens` executable. It has two modes: `scan` for a disk usage tree and `clean` for selecting cleanup candidates and deleting them after confirmation. In Scan mode, `d` deletes the current file or directory after confirmation, while `A` deletes all discovered preset candidates at once. Preset candidates are highlighted in red, and scan rows support mouse focus and folder expansion. It runs on Node.js 20 or newer:

```bash
yarn tui ~/Dev --preset rust
yarn tui ~/Dev --preset node,gitignored --sort path
npx space-lens ~/Dev --preset rust
```

Inside the TUI, press `tab` to switch modes, `space` to select a cleanup candidate, `x` to request deletion, and `enter` to confirm. In Scan mode, use `d` for the current row or `A` for all preset candidates. Use `Ctrl+C` or `q` to quit.

## Benchmark CLI

This repository includes a local CLI for benchmarking the directory scanner and exporting trees:

```bash
yarn bench ~/Dev
yarn bench ~/Dev --no-json-size
yarn bench ~/Dev --export-tree tree.json
```

Options:

```text
--export-tree PATH
--json-size / --no-json-size
--ignore-hidden
--full-path
--respect-gitignore / --no-respect-gitignore
--ignored-mode summarize|exclude
```

## Development

```bash
yarn install
yarn workspace space-lens build:debug
yarn workspace @space-lens/cli build
yarn workspace space-lens test
yarn workspace @space-lens/cli test
yarn workspace space-lens typecheck
cargo test --workspace
```

Useful local commands:

- `yarn build`: build release bindings for the current platform.
- `yarn build:debug`: build debug bindings for local testing.
- `yarn tui`: run the Solid/Uniview cleanup candidate viewer.
- `yarn test`: run Rust workspace tests and AVA tests.
- `yarn typecheck`: type-check the TypeScript workspaces.
- `yarn bench`: run the benchmark CLI from the `space-lens` npm workspace.

## iCloud Free macOS app

The SwiftUI app lives in `apps/icloud-free`. From the repository root, the convenience recipes are:

```bash
just icloud-build       # release build and signed local .app bundle
just icloud-open        # open the existing bundle
just icloud-build-open  # build, then open
just icloud-test        # run Swift tests
```

Use `CONFIGURATION=debug just icloud-build` for a debug bundle. The app bundle is written to `apps/icloud-free/.build/ICloudFree.app` and is not installed into `/Applications` automatically.

## Space Lens native macOS prototype

The native Space Lens macOS app lives in `apps/space-lens-mac`. It keeps a
synthetic demo map for safe UI/performance checks and now supports a read-only
real-folder scan through the separate Rust FFI facade. The scan can be
exported as the portable JSON snapshot. Normal filesystem cleanup uses an
explicit review flow that moves selected real nodes to the macOS Trash, so
they remain recoverable through Finder; it does not permanently delete files.
The optional MCP adapter is read-only. The macOS app also exposes the Apple-only iCloud Local Copies
capability and its bounded concurrent eviction flow; it never requests
downloads for cloud-only files.

```bash
just space-lens-mac-test
just space-lens-mac-build
just space-lens-mac-open
```

The portable snapshot contract is in `packages/space-lens/src/snapshot.rs`; the
separate C ABI facade and header are in `packages/space-lens-ffi`. The FFI
facade exposes read-only filesystem snapshots plus an opaque macOS-only iCloud
plan session. The native app can execute that session only after its explicit
confirmation flow; MCP remains read-only and does not expose the execution API.

### Optional Rust MCP adapter

The CLI has an opt-in, read-only MCP stdio adapter. The default Rust build does
not compile or expose it; enable the `mcp` feature explicitly:

```bash
cargo run --release -p spacelens --features mcp -- mcp
# or
just space-lens-mcp
```

It implements the MCP initialize lifecycle and `tools/list`/`tools/call` for
filesystem snapshot export, cleanup-candidate inspection, platform
capabilities, and read-only iCloud plans. It exposes no delete, Trash, cloud
eviction, or download tool. MCP messages use newline-delimited JSON-RPC on
stdin/stdout; diagnostics must not be written to stdout.

## Web UI status

The current `main` checkout has no `apps/web` workspace and no web start script. The Svelte Web UI and its standalone/Kunkun hosts live on the separate `kunkun-ext` branch. Do not switch the current dirty worktree just to run it; use a separate worktree and follow that branch's README instructions.

