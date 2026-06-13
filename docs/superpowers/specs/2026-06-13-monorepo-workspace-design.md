# Space Lens Monorepo Workspace Design

Date: 2026-06-13

## Summary

Space Lens will become a mixed Rust and JavaScript monorepo with two top-level product areas:

- `packages/` contains reusable libraries and bindings.
- `apps/` contains executable products and user-facing tools.

The project will replace the older `devclean` direction by moving new scan, cleanup, CLI, TUI, desktop, and HTTP UI work into this repository. The initial migration will create a pure Rust core package, move the current NAPI package into a package subdirectory, and add a simple CLI app.

## Goals

- Provide a pure Rust library that can be published to crates.io.
- Keep the existing npm package name `space-lens` as the Node.js NAPI binding.
- Move NAPI-specific code out of the Rust core so Rust users do not inherit NAPI build constraints.
- Add a simple executable CLI as the first app-level consumer of the Rust core.
- Preserve the existing multi-platform NAPI build, test, and npm trusted publisher release flow after moving the npm package into a subdirectory.
- Establish a workspace shape that can later support an NPX app, TUI, desktop app, and HTTP UI without rewriting core scanning logic.

## Non-Goals

- Do not implement the desktop app, TUI, or HTTP UI in the first migration.
- Do not replace Yarn with pnpm.
- Do not keep `devclean` as a parallel maintained project.
- Do not make cleanup destructive by default.
- Do not couple the Rust core to CLI prompts, progress bars, NAPI types, or UI-specific formatting.

## Repository Layout

```txt
space-lens/
  Cargo.toml
  package.json
  yarn.lock
  .yarnrc.yml

  packages/
    space-lens/
      Cargo.toml
      src/
        lib.rs
        scanner.rs
        rules.rs
        clean.rs

    node/
      Cargo.toml
      package.json
      build.rs
      src/
        lib.rs
      index.js
      index.d.ts
      __test__/
      benchmark/
      scripts/

  apps/
    cli/
      Cargo.toml
      src/
        main.rs

    npx/
      package.json
      src/
        cli.ts
```

`packages/space-lens` is the canonical core library. `packages/node` is the npm NAPI binding. `apps/cli` is a simple Rust CLI. `apps/npx` is reserved for a future executable npm package; it is placed under `apps/` because it is an executable product even though npm packaging is required for `npx`.

## Package Identities

- crates.io core package: `space-lens`
- Rust crate import name: `space_lens`
- npm library package: `space-lens`
- Rust CLI binary name: `space-lens`
- future npm executable package: `@space-lens/cli` or `space-lens-cli`

The Rust and npm package registries are separate, so using `space-lens` in both crates.io and npm is acceptable. The NAPI package will remain the owner of the existing npm package name.

## Dependency Direction

```txt
packages/space-lens
  <- packages/node
  <- apps/cli
  <- future apps/desktop
  <- future apps/tui

packages/node
  <- future apps/npx
```

The core package must not depend on any app or binding package. All consumers adapt the core data model into their own output format.

## Core Library Design

The core library will expose read-only scan operations and explicit cleanup planning:

- `scan_directory` builds a size tree for one or more paths.
- `find_candidates` finds cleanup candidates using presets and custom rules.
- `build_removal_plan` converts candidates into a dry-run plan with sizes and paths.
- `execute_removal_plan` performs deletion only when explicitly requested by the caller.

The core data model includes:

- scan options: roots, hidden file behavior, gitignore behavior, ignored path mode, output path style
- scan nodes: path, display name, size, depth, ignored flag, collapsed flag, children
- cleanup presets: node, rust, gitignored
- cleanup candidates: path, size, reason, preset or rule source, safety metadata
- removal plan: dry-run entries, total bytes, errors, skipped paths

The core should use structured Rust types and `serde` where useful so Node bindings, CLIs, and future UI apps can serialize results consistently.

## Cleanup Behavior

Cleanup must be safe by default.

- `scan` and `candidates` commands never delete files.
- `clean` defaults to dry-run.
- Actual deletion requires an explicit execution flag in every app-level interface.
- Gitignored cleanup must report the matched path and the reason before deletion.
- The core should separate candidate discovery from deletion so UI apps can review and filter targets before executing.

Initial presets:

- `node`: reports `node_modules`.
- `rust`: reports Cargo `target` directories.
- `gitignored`: reports ignored paths discovered from `.gitignore` rules.

The design keeps presets extensible without hard-coding all behavior into the CLI.

## NAPI Package Design

`packages/node` will contain the NAPI crate and npm package. It depends on `packages/space-lens` through the Cargo workspace and exposes JavaScript-friendly types.

The NAPI wrapper should remain thin:

- convert JS options into core Rust options
- call core APIs
- convert core output into NAPI object types
- avoid implementing scanning, rule matching, or deletion logic in the binding layer

The existing generated `index.js`, `index.d.ts`, platform package behavior, and NAPI artifacts will move with the package.

## CLI App Design

The first CLI can be simple and Rust-based. It consumes `packages/space-lens` directly.

Initial commands:

```txt
space-lens scan <path...>
space-lens candidates <path...>
space-lens clean <path...>
```

Initial flags:

```txt
--preset node
--preset rust
--preset gitignored
--json
--execute
```

`--execute` is valid only for `clean`. Without `--execute`, `clean` prints a dry-run removal plan.

## Yarn Workspace Design

The root `package.json` becomes a private Yarn workspace root. Yarn stays on the current version family and keeps `nodeLinker: node-modules`.

Workspace members:

```json
[
  "packages/node"
]
```

`packages/node` remains publishable. `apps/npx` can be added as a workspace member when the NPX app is implemented. Rust-only packages do not need npm workspace entries.

## Cargo Workspace Design

The root `Cargo.toml` becomes a Cargo workspace root.

Workspace members:

```toml
[
  "packages/space-lens",
  "packages/node",
  "apps/cli"
]
```

The root will no longer be the NAPI crate. The NAPI crate moves to `packages/node`.

## CI and Publish Design

CI must be updated because the NAPI package moves from repository root to `packages/node`.

Required changes:

- Run Yarn install from the repository root.
- Run NAPI build, test, artifact movement, and npm publish commands with the NAPI package working directory.
- Upload and download NAPI artifacts relative to `packages/node`.
- Keep npm trusted publisher OIDC and provenance enabled.
- Keep npm trusted publisher settings pointed at `.github/workflows/CI.yml`; run `npm publish` from `packages/node`.
- Run Rust formatting, clippy, and tests across the Cargo workspace.
- Add CLI build or smoke test for `apps/cli`.

The current tag-driven publish model remains:

- `vX.Y.Z` tags publish stable npm packages.
- prerelease tags publish with the `next` npm tag.

Publishing crates.io can be added after the core package is stable enough for the first crate release.

## Testing

Initial verification should include:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets`
- `cargo test --workspace`
- Yarn install from root
- NAPI build from `packages/node`
- NAPI tests from `packages/node`
- CLI smoke test against a temporary fixture

Tests should cover:

- existing scan behavior
- hidden file handling
- gitignore summarize and exclude modes
- hardlink deduplication behavior
- cleanup candidate discovery for node, rust, and gitignored presets
- dry-run removal plan behavior

## Migration Plan

1. Create root Cargo and Yarn workspace configuration.
2. Move the current pure scanner logic into `packages/space-lens`.
3. Move the current NAPI package into `packages/node`.
4. Convert the NAPI wrapper to call the core package.
5. Add `apps/cli` as a simple Rust CLI using the core package.
6. Update CI paths, working directories, artifact paths, and publish commands.
7. Verify Rust workspace tests and NAPI package tests locally.
8. Leave future desktop, TUI, HTTP UI, and NPX app work out of the first implementation.

## Approval State

The approved direction is a direct monorepo migration using `packages/` and `apps/`, without a separate `crates/` folder. Library-style reusable code belongs in `packages/`; executable products belong in `apps/`.
