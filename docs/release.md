# Release Process

How every Space Lens artifact gets out the door. Last verified 2026-09-26
(spacelens 0.2.9 / kuntu-scan 0.2.0).

## Version layout

| Artifact | Version lives in | Published to |
| --- | --- | --- |
| Engine library `kuntu-scan` | `vendors/kuntu` → `crates/kuntu-scan/Cargo.toml` | crates.io |
| CLI `spacelens` (bin `spacelens`) | root `Cargo.toml` `[workspace.package] version` | crates.io + GitHub release + Homebrew tap |
| npm package `space-lens` | `packages/node/package.json` | npm |
| Desktop app | Tauri config, tagged `app-v*` | GitHub release (dmg) via `workbench.yml` |

The workspace Cargo version and the npm package.json version are **independent
files** — bump the one you are releasing. Note the tag prefixes are
component-scoped: `cli-v*` (CLI), `app-v*` (desktop), `kfs-v*` (misc). Plain
`v*` tags are legacy repo-wide releases and also trigger the npm CI workflow —
do not use them for the CLI.

## 1. Engine change (kuntu-scan)

1. Work in `vendors/kuntu` (it is a git submodule of github.com/HuakunShen/kuntu).
   Branch off `origin/main`, commit, push, open a PR, merge.
2. In space-lens, move the submodule pin onto kuntu `main`:
   `cd vendors/kuntu && git checkout main && git pull`, then
   `git add vendors/kuntu` from the space-lens root and commit.
3. Only if the engine API/behavior changed in a way crates.io consumers need:
   bump `crates/kuntu-scan/Cargo.toml` and `cargo publish -p kuntu-scan`
   (dry-run first: `cargo publish --dry-run -p kuntu-scan`). Do this **before**
   publishing `spacelens` if `apps/cli` will need the new version — the CLI's
   dependency is `kuntu-scan = { version = "0.2", path = ... }`, and
   `cargo publish` resolves the registry version.

## 2. CLI (`spacelens`) release — the common case

```bash
# a) bump the workspace version in root Cargo.toml, commit, push
cargo publish -p spacelens                      # b) crates.io (token via cargo login)
git tag cli-v0.2.10 && git push origin cli-v0.2.10   # c) CI builds & attaches archives
```

The tag push runs `.github/workflows/cli-release.yml`, which builds
aarch64/x86_64 macOS (x64 cross-compiled on arm runners), aarch64/x86_64 Linux
musl, and Windows; archives are named
`spacelens-<target>-v<version>.tar.gz` (`.zip` on Windows) and attached to the
GitHub release. That naming is what `cargo binstall spacelens` resolves via
`[package.metadata.binstall]` in `apps/cli/Cargo.toml` — if you rename the
archives, rename the metadata too.

Then update Homebrew (fill shas from the release assets):

```bash
for t in aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-musl x86_64-unknown-linux-musl; do
  echo "$t $(shasum -a 256 <(curl -sL "https://github.com/HuakunShen/space-lens/releases/download/cli-v<version>/spacelens-$t-v<version>.tar.gz") | cut -d' ' -f1)"
done
```

Edit `packaging/homebrew/Formula/spacelens.rb` (version + the four sha256
values), commit here, then copy the file into the tap and push:

```bash
cp packaging/homebrew/Formula/spacelens.rb "$TAP_CLONE/Formula/spacelens.rb"
cd "$TAP_CLONE" && git commit -am "spacelens <version>" && git push
```

Verify locally: `brew reinstall HuakunShen/homebrew-tap/spacelens && spacelens --version`.
(`brew audit` is nice but optional.)

## 3. npm release

1. Bump `packages/node/package.json` version (keep it in sync with the
   workspace Cargo version when both ship together).
2. Trigger the `npm-cli.yml` workflow (workflow_dispatch, choose dist-tag,
   default `latest`). CI builds the napi binaries for each platform and
   publishes with provenance; it skips itself if the version already exists.

Note: pushing a plain `v*` tag also arm CI.yml's npm publish job (it skips if
`npm view space-lens@<version>` already exists). Prefer `npm-cli.yml`.

## Gotchas learned the hard way

- **macOS packaging**: BSD `install` has no `-D`; use `mkdir -p` +
  `install -m 755`. The packaging step in cli-release.yml is portable — keep it that way.
- **macos-13 (Intel) runners are starved**; x86_64-apple-darwin is
  cross-compiled on `macos-15`. Don't switch back.
- **Artifacts**: upload only `spacelens-*.tar.gz|zip` — the staging directory
  next to the archive matches `spacelens-*` and makes
  `gh release create` fail mid-upload (and it rolls back the release).
- **Homebrew 7** strips a tarball's single root directory when unpacking; the
  formula accepts both layouts. Newer brew also requires
  `brew trust huakunshen/tap` before installing from the tap.
- `cargo publish` needs a token (`cargo login`) on the machine doing it.
- Always `--dry-run`/no-`--execute` first: `spacelens clean` deletes for real
  only with `--execute`.
