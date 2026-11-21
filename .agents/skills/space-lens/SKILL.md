---
name: space-lens
description: Use the Space Lens project — scan disk usage, find cleanup candidates (node_modules, target, gitignored), clean them, and discover dirty git repos — via the npm library, the Rust `spacelens` CLI, the Rust `kuntu-scan` library, the MCP stdio adapter for AI clients, or `spacelens remote` over SSH (Linux/macOS/Windows targets, trash-only remote cleanup). Covers install paths (brew, cargo binstall, cargo, npm), the engine/submodule layout, and where releases are documented.
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
spacelens remote scan ufo --path ~        # scan an SSH host from ~/.ssh/config
spacelens remote cleanup ufo --path ~/dev --execute  # move remote caches to trash
spacelens remote open ufo                  # browser URL for the full workbench
spacelens mcp                              # read-only MCP stdio server
spacelens mcp --allow-cleanup              # …with local + remote cleanup unlocked
```

- MCP ships in the prebuilt brew/binstall binaries (source builds need
  `--features mcp`); MCP clients use command `spacelens`, args `["mcp"]`.

- `--preset` accepts `node` (`node_modules`), `rust` (`target`),
  `gitignored` (anything matched by .gitignore). No preset = all three.
- `clean` is **dry-run by default**; deletion needs `--execute`.
- `--json` on every command for machine-readable output.
- **Symlinks are not followed by default.** Symlinked external projects
  (vendored checkouts, `references/*` style dirs) are measured as link nodes.
  Pass `--follow-symlinks` to descend into them; an inode guard prevents
  symlink cycles and double counting.
- `clean`/`candidates` skip hidden dirs only with `--ignore-hidden`.

## Remote SSH hosts

`spacelens remote <command> <target>` drives any host you can reach with your
own OpenSSH — a `~/.ssh/config` alias or `user@host[:port]`, on Linux, macOS,
or Windows 11. The first connect installs the pinned Rust agent and the npm
host under the remote `~/.space-lens` (no sudo, no global npm); every command
stops the remote server when it ends. Remote cleanup is **trash-only**: the
host refuses permanent deletion by construction.

- Commands: `list` (ssh-config hosts), `probe`, `scan --path`, `discovery
  --scan-id`, `cleanup --path [--execute]`, `open`. With no target and no
  path, `scan`/`cleanup` turn interactive: a numbered host list, a path
  prompt showing the home's size, and `b` to drill real size-sorted rows
  (browsing a >50 GiB root asks twice — the tree loads on the remote host).
- The remote needs Node.js ≥ 20 and npm (nvm/Homebrew/user-local installs are
  found without an interactive PATH). Connect once with plain `ssh <target>`
  to trust the host key: the channel is strict (`BatchMode`,
  `StrictHostKeyChecking=yes`).
- A scan id dies with its server, so `remote cleanup` derives everything in
  one session: it scans, discovers (default `caches`), plans, and only
  `--execute` moves the planned nodes into the remote trash.
- `remote open` prints a browser URL for the full workbench (a single-use
  ticket minted for the tunnel's own loopback origin). Keep the command
  running — it holds the SSH tunnel; the remote host is reaped on the next
  connect after it exits. That browser session is read-only; use
  `remote cleanup` (or MCP with `--allow-cleanup`) to trash files.
- The desktop app's **Switch device → SSH hosts** connects the same way and
  does support cleanup: review a plan, then **Move to Trash** into the remote
  trash. A remote device scans without `localOnly` (a protected scan is
  read-only by contract, so the host would refuse the plan).
- Windows specifics: commands travel as one PowerShell `-EncodedCommand`
  token (its OpenSSH default shell is cmd.exe); files travel by scp, because
  a PowerShell child never sees an stdin EOF; Windows remotes serve without
  the memory view (`--allow-memory` is macOS/Linux only).
- Windows cleanup refuses rather than deleting permanently: the shell's
  recycle API falls back to a permanent delete when it cannot recycle (a
  non-interactive SSH session does this) and still returns success, so the
  host probes the bin with a zero-byte file first and verifies each item
  landed in it (0.4.1).
- Before a pinned release exists, run the same pipeline against local
  artifacts: `SPACELENS_REMOTE_AGENT=<binary>`,
  `SPACELENS_REMOTE_HOST_TARBALL=<npm pack tarball>`, and optionally
  `SPACLENS_REMOTE_MANIFEST=<manifest.json>`.

## MCP adapter for AI clients

`spacelens mcp [--allow-cleanup]` speaks MCP over stdio. **No tool call ever
blocks on a long scan**: `space_lens_scan` returns a scanId immediately
(local scans run in a background thread, remote scans on the provisioned
host), the client polls `space_lens_scan_status`, and navigates with
`space_lens_ls` — one folder level at a time, size-sorted, paginated via
`limit`/`offset`. `space_lens_devices` lists this machine plus every
`~/.ssh/config` host; every tool accepts `device` (omit or `"local"` for
this machine, an ssh alias for a remote). Sessions stay connected for the
process lifetime and every remote server is reaped on exit.

Default is read-only. `--allow-cleanup` unlocks `space_lens_cleanup_execute`
(local paths, real deletion) and `space_lens_remote_cleanup_plan` /
`space_lens_remote_cleanup_execute` (remote, trash-only). Without the flag
every cleanup tool answers `Forbidden`.

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
