# CLI and TUI

**Updated: 2026-09-23** — added the feature-gated `mcp` subcommand
(`apps/cli/src/mcp.rs`, working tree as of this sync) and the
`spacelens serve` machine mode.

## Rust CLI (`apps/cli`, package `space-lens-cli`)

```bash
cargo run -p space-lens-cli -- scan ~/Dev --json
cargo run -p space-lens-cli -- candidates ~/Dev --preset node
cargo run -p space-lens-cli -- clean ~/Dev --preset node      # --execute to act
cargo run -p space-lens-cli -- icloud inspect <abs-path>      # plan/evict too
cargo run --release -p space-lens-cli --features mcp -- mcp   # just space-lens-mcp
```

- `clean` defaults to dry-run; `icloud evict` requires `--execute` plus the
  interactive confirmation phrase and never falls back to deletion.
- **MCP adapter (new, `--features mcp`)**: read-only stdio JSON-RPC server,
  protocol `2025-11-25`; exposes scan, cleanup-candidate plans, snapshots
  (`SnapshotEnvelope`), and iCloud eviction planning. Enabled by
  `just space-lens-mcp`. *Not yet on `main` at the pinned wiki revision.*

## TUI (`apps/tui`, workspace `@space-lens/cli`)

Two modes, `tab` switches: `scan` (disk-usage tree) and `clean` (cleanup
candidates). Keys: `space` select · `x` delete request · `enter` confirm ·
`d` delete current row · `A` delete all preset candidates · `q`/`Ctrl+C`
quit. Preset candidates render red; scan rows support mouse focus and folder
expansion. Node ≥ 20.

```bash
yarn tui ~/Dev --preset node,gitignored --sort path
npx space-lens ~/Dev --preset rust        # same TUI via the npm bin
```

## Serve CLI (npm bin `spacelens`, `packages/node/cli.ts`)

```bash
npx spacelens serve                      # loopback, read-only, cwd
npx spacelens serve --host 0.0.0.0 --allow-cidr 192.168.1.0/24
npx spacelens serve --allow-cleanup      # grant trash-based cleanup
npx spacelens serve --machine --port 0 --json --no-open   # VS Code supervisor mode
```

## just recipes

`rust-cli`, `space-lens-mcp`, `icloud-build`/`icloud-test`/`icloud-cli`,
`space-lens-mac-test`/`-build`/`-open`.
