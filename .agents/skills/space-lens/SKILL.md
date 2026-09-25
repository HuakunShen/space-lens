---
name: space-lens
description: Use the Space Lens project — scan disk usage, find cleanup candidates (node_modules, target, gitignored), clean them, and discover dirty git repos — via the npm library, the Rust `spacelens` CLI, or the Rust `kuntu-scan` library. Covers install paths (brew, cargo binstall, cargo, npm), the engine/submodule layout, and where releases are documented.
---

# Space Lens

Fast directory scanning and cleanup candidates, powered by the Rust engine
[kuntu-scan](https://github.com/HuakunShen/kuntu) (vendored here at
`vendors/kuntu` as a git submodule) with product shells around it.

| Surface | Location | Consume via |
| --- | --- | --- |
| npm library + Node TUI | `packages/node` (napi) | `npm install space-lens` |
| Rust CLI (`spacelens` bin) | `apps/cli` (crate `spacelens`) | `brew` / `cargo binstall` / `cargo install` |
| SwiftUI desktop, web workbench, VS Code, kunkun plugin, iCloud app | `apps/*` | see README |

The engine crate is published to crates.io as `kuntu-scan`; the CLI crate is
`spacelens`. Both names are also the installed command/binary names — do not
confuse the npm bin `space-lens` (TUI wrapper) with the Rust CLI `spacelens`.

## Install the CLI

```bash
brew install HuakunShen/homebrew-tap/spacelens   # prebuilt, fastest
cargo binstall spacelens                          # prebuilt via cargo-binstall
cargo install spacelens                           # from source (crates.io)
```

First-time users of a third-party tap on Homebrew ≥ 4.60 may need
`brew trust huakunshen/tap`.

## CLI usage

`spacelens <command>` — every command takes one or more PATHs (default `.`).

```bash
spacelens scan ~/Dev                       # sized directory tree
spacelens candidates ~/Dev --preset node   # list cleanup candidates (dry run)
spacelens clean ~/Dev --preset node        # print removal plan (dry run!)
spacelens clean ~/Dev --preset node --execute  # actually delete
spacelens dirty-git ~/Dev                  # git repos with uncommitted changes
spacelens icloud plan|inspect|evict <path> # iCloud Drive eviction (macOS)
spacelens mcp                              # MCP server (built with --features mcp)
```

- `--preset` accepts `node` (`node_modules`), `rust` (`target`),
  `gitignored` (anything matched by .gitignore). No preset = all three.
- `clean` is **dry-run by default**; deletion needs `--execute`.
- `--json` on every command for machine-readable output.
- **Symlinks are not followed by default.** Symlinked external projects
  (vendored checkouts, `references/*` style dirs) are measured as link nodes.
  Pass `--follow-symlinks` to descend into them; an inode guard prevents
  symlink cycles and double counting.
- `clean`/`candidates` skip hidden dirs only with `--ignore-hidden`.

## npm library usage

```ts
import {
  scanDirectory, findCleanupCandidates, planCleanup,
  executeCleanup, findDirtyGitRepos,
} from 'space-lens'

const tree = scanDirectory({
  directories: [process.cwd()],
  ignoreHidden: false,
  respectGitignore: true,
  ignoredMode: 'summarize',   // ignored dirs collapse into sized leaves
  followSymlinks: false,
})

const candidates = findCleanupCandidates({
  directories: [process.cwd()],
  presets: ['node', 'rust', 'gitignored'],
  followSymlinks: false,
})

const plan = planCleanup({ directories: [process.cwd()], presets: ['node'] })
const outcome = executeCleanup(plan)   // real deletion — plan first!

const dirty = findDirtyGitRepos({ directories: ['~/Dev'], ignoreHidden: true })
// => [{ path, dirtyEntries }]
```

`followSymlinks` defaults to `false` everywhere; opt in explicitly.

## Rust library usage

Depend on `kuntu-scan` (crates.io). The same options as above exist as
`ScanOptions`, `CandidateOptions` (`follow_symlinks: bool`, serde default
false) and `DirtyGitRepoOptions`; see `scan_directory`, `find_candidates`,
`build_removal_plan`, `execute_removal_plan`, `find_dirty_git_repos`.
The iCloud eviction planner is `kuntu_scan::cloud` (macOS only).

## Repository layout for contributors

- Engine changes go to the **kuntu repo** (`vendors/kuntu` submodule): branch →
  PR → merge → bump the submodule pin here. `kuntu-napi` only exposes the file
  search index; the scan/cleanup Node bindings live in `packages/node/src/lib.rs`.
- Rust CLI: `apps/cli` (clap; depends on kuntu-scan by version + path override).
- Adding a cleanup preset or option means touching: kuntu-scan →
  `packages/node/src/lib.rs` + `index.d.ts` → `apps/cli`.
- Release process: see [`docs/release.md`](../../../docs/release.md).
