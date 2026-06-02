# Kunkun File Search

Rust-only file search prototype for Kunkun. This workspace is intentionally independent from Electron, Tauri, Node, and the desktop app database.

## Crates

- `kfs-core`: shared search types, path policy, explanation, and ranking.
- `kfs-crawler`: explicit-root filesystem crawler that applies `kfs-core` policy.
- `kfs-index-sqlite`: persistent metadata index, incremental refresh, repair, and search over crawled entries.
- `kfs-watcher`: platform-neutral watcher event model with bounded polling maintenance.
- `kfs-provider-spotlight`: macOS Spotlight provider backed by `mdfind`.
- `kfs-cli`: local CLI adapter for search, explain, index, watch, and benchmark commands.

## Safety

Search roots are explicit. Do not run broad full-disk searches while developing this prototype. Manual smoke tests should stay within:

- `~/Desktop`
- `~/Downloads`
- `~/Dev`

Sensitive paths such as `.ssh`, `.aws`, `.gcloud`, `.kube`, `.docker`, `.env`, private keys, credentials, and secrets are denied by default.

## Examples

```bash
cargo run -p kfs-cli -- explain ~/Dev --root ~/Dev
cargo run -p kfs-cli -- search "package json" --root ~/Dev --limit 5 --json
cargo run -p kfs-cli -- index rebuild --root . --db /tmp/kfs.sqlite
cargo run -p kfs-cli -- index refresh --root . --db /tmp/kfs.sqlite
cargo run -p kfs-cli -- watch --root . --db /tmp/kfs.sqlite --duration-ms 1000
cargo run -p kfs-cli -- bench "Cargo toml" --root . --provider sqlite --db /tmp/kfs.sqlite
cargo run -p kfs-cli -- search "Cargo toml" --root . --provider sqlite --db /tmp/kfs.sqlite --json
```

If Spotlight returns an empty array for a scoped search, the provider path is still functioning; it usually means that macOS has not indexed that root or has no matching filename/path metadata for the query. The core filter/ranker can still be tested through unit tests and provider fixtures.
