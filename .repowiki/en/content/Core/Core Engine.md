# Core Engine

**Updated: 2026-09-23** — reflects the Kuntu engine move (commit 02869e7) and
the bounded eviction work in the current tree.

## Where the engine lives

The scanner/snapshot/cleanup/iCloud-eviction engine moved out of this repo to
Kuntu's `kuntu-scan` crate (tag `v0.3.0`), consumed here as a git submodule
at `vendors/kuntu` (pinned in `.gitmodules`). The crate dependency is
**aliased as `space_lens`**, so `space_lens::` call sites are unchanged.
Sibling vendored crates: `kuntu-core`, `kuntu-index`, `kuntu-watcher`,
`kuntu-napi`, `kuntu-cli`, and others under `vendors/kuntu/crates/`.

## Capabilities

- **Scanning** — `scan_directory` with `ScanOptions`; `IgnoredMode`:
  `summarize` collapses ignored trees (e.g. `target/`, `node_modules/`) to
  sized leaves, `exclude` skips them.
- **Cleanup** — `find_candidates` + `build_removal_plan` (dry-run first;
  presets `node`, `rust`, `gitignored`).
- **iCloud local copies** — `space_lens::cloud` with `NativeICloudBackend`
  and `build_eviction_plan`; bounded worker pool (8 concurrent by default,
  capped at 32) — never one task per file, never serial.
- **Snapshot** — portable JSON snapshot contract in
  `packages/space-lens/src/snapshot.rs`; `SnapshotEnvelope` is shared by the
  NAPI package, the C ABI, and the MCP adapter.

## Bindings out of the engine

| Binding | Path | Consumers |
| ------- | ---- | --------- |
| napi-rs | `packages/node/src/lib.rs` (+ `cloud.rs`) | npm package, TUI, Serve Host |
| C ABI | `packages/space-lens-ffi` (`include/space_lens_ffi.h`) | `apps/space-lens-mac` |
| Direct Rust | `space_lens::` dependency | `apps/cli`, Tauri commands |

## Implementation notes

- **iCloud execution has no delete fallback** (critical): eviction uses
  `FileManager.evictUbiquitousItem` semantics only; plan execution re-verifies
  every item and stays dry-run until `--execute` + confirmation phrase.
- The macOS app never requests downloads for cloud-only files.
- `just space-lens-mac-test` builds `space-lens-ffi` first, then runs Swift
  tests with `SPACE_LENS_FFI_LIB_DIR`.
