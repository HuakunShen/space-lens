# CI and Release

**Updated: 2026-09-23** — homebrew tap publishing + macOS x64 cross-compile
target fixes landed at `5d74b29`.

Three workflows under `.github/workflows/`, split by release line:

## `CI.yml` — npm/TUI line (`v*` tags + push to `main`)

- **Lint job**: Node lint + typecheck, `cargo fmt --check`, clippy
  `--workspace --all-targets`, `cargo test --workspace`, debug binding
  build, AVA tests for `space-lens` and `@space-lens/cli`.
- **Build matrix**: 8 napi targets (darwin/linux-gnu/linux-musl × x64+arm64,
  windows x64/arm64 — the macOS x64 cross-compile target set was fixed in
  `5d74b29`).
- `paths-ignore`: `.md`, `LICENSE`, `.gitignore`, `.editorconfig`,
  `docs/**`. Docs-only pushes don't trigger CI.

## `npm-cli.yml` — publish `@space-lens/cli`

Triggers: `v*` / `cli-v*` tags or manual dispatch with dist-tag choice
(`latest`/`next`); npm trusted publishing (`id-token: write`, no token
secret).

## `workbench.yml` — multi-runtime line (`app-v*` tags + PRs)

- **js**: contract artifacts byte-fresh, client/host unit tests, typecheck every
  JS workspace, web build (browser **and** desktop flavors), wrangler deploy
  dry-run, VS Code extension compile.
- **desktop-test**: cargo test matrix macOS / ubuntu-22.04 / Windows.
- `app-v1.2.3` builds desktop bundles **and** the vsix together; npm keeps its
  own `v*` tags.

## Distribution

- **Homebrew**: cask source of truth `packaging/homebrew/Casks/space-lens.rb`
  → pushed to `HuakunShen/homebrew-tap` per release with real version +
  sha256 (`scripts/render-cask.py`, commit `5d74b29`):
  `brew install --cask HuakunShen/homebrew-tap/space-lens`.
- **Updater**: Tauri updater checks `releases/latest/download/latest.json`,
  minisign-verified (public key in `tauri.conf.json`).
- **npm**: `space-lens` platform packages from `CI.yml`; `@space-lens/cli`
  from `npm-cli.yml`.
