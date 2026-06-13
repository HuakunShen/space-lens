# Development Guidelines

<cite>
**Referenced Files in This Document**
- [.husky/pre-commit](file:///Users/hk/Dev/space-lens/.husky/pre-commit)
- [.editorconfig](file:///Users/hk/Dev/space-lens/.editorconfig)
- [rustfmt.toml](file:///Users/hk/Dev/space-lens/rustfmt.toml)
- [.taplo.toml](file:///Users/hk/Dev/space-lens/.taplo.toml)
- [.github/workflows/CI.yml](file:///Users/hk/Dev/space-lens/.github/workflows/CI.yml)
- [package.json](file:///Users/hk/Dev/space-lens/package.json)
- [packages/node/package.json](file:///Users/hk/Dev/space-lens/packages/node/package.json)
- [AGENTS.md](file:///Users/hk/Dev/space-lens/AGENTS.md)
</cite>

## Table of Contents

1. [Code Formatting](#code-formatting)
2. [Linting](#linting)
3. [Pre-commit Hooks](#pre-commit-hooks)
4. [Testing Discipline](#testing-discipline)
5. [CI Gates](#ci-gates)
6. [Branch & Commit Conventions](#branch--commit-conventions)
7. [Dependency Management](#dependency-management)

## Code Formatting

All code is formatted via automated tools — never manually format:

| Format | Tool | Config | Check |
|--------|------|--------|-------|
| Rust | `cargo fmt` | `rustfmt.toml` — 2-space tabs | CI: `cargo fmt --all -- --check` |
| TypeScript/JS/JSON/YAML | `prettier` | 120 print width, no semi, single quotes | Via lint-staged |
| TOML | `taplo format` | `.taplo.toml` — align entries, reorder keys | Via lint-staged |

Root command: `yarn format` (runs all three formatters).

**Section sources**

- [rustfmt.toml](file:///Users/hk/Dev/space-lens/rustfmt.toml#L1)
- [.taplo.toml](file:///Users/hk/Dev/space-lens/.taplo.toml#L1-L7)
- [package.json](file:///Users/hk/Dev/space-lens/package.json#L16-L19)

## Linting

The project uses **oxlint** instead of eslint for TypeScript/JavaScript:

```bash
yarn lint                    # oxlint (node) + tsc --noEmit (tui)
yarn workspace space-lens lint   # oxlint only
yarn workspace @space-lens/cli lint  # tsc --noEmit only
```

For Rust: `cargo clippy --workspace --all-targets` (run in CI, not in pre-commit).

**Section sources**

- [packages/node/package.json](file:///Users/hk/Dev/space-lens/packages/node/package.json#L58)
- [.github/workflows/CI.yml](file:///Users/hk/Dev/space-lens/.github/workflows/CI.yml#L46-L53)

## Pre-commit Hooks

The Husky pre-commit hook runs `yarn lint-staged`:

```bash
# .husky/pre-commit
yarn lint-staged
```

`lint-staged` configuration is in `packages/node/package.json`:
- `*.@(js|ts|tsx)` → `oxlint --fix`
- `*.@(js|ts|tsx|yml|yaml|md|json)` → `prettier --write`
- `*.toml` → `taplo format`

**Section sources**

- [.husky/pre-commit](file:///Users/hk/Dev/space-lens/.husky/pre-commit)
- [packages/node/package.json](file:///Users/hk/Dev/space-lens/packages/node/package.json#L84-L93)

## Testing Discipline

- All tests must pass before merging (CI gate).
- Three test frameworks coexist: `cargo test` (Rust), AVA (packages/node), node:test (apps/tui).
- AVA tests are `.ts` files transpiled by `@oxc-node/core/register` — not tsx or ts-node.
- NAPI binding must be built (`yarn build:debug`) before running JS tests.
- TUI tests use `node --import tsx` for TypeScript transpilation.

Run focused: `yarn workspace <name> test`.

**Section sources**

- [packages/node/package.json](file:///Users/hk/Dev/space-lens/packages/node/package.json#L95-L108)
- [apps/tui/package.json](file:///Users/hk/Dev/space-lens/apps/tui/package.json#L33)
- [AGENTS.md](file:///Users/hk/Dev/space-lens/AGENTS.md)

## CI Gates

The CI pipeline (`.github/workflows/CI.yml`) enforces:

1. **Lint job**: rustfmt check, clippy, oxlint, tsc --noEmit, cargo test, JS tests
2. **Build matrix**: 8 native targets across 3 OSes + 2 architectures
3. **Binding tests**: Download artifacts and test on each platform with Node 22 & 24
4. **Publish**: Only on version tags, with npm provenance

**Section sources**

- [.github/workflows/CI.yml](file:///Users/hk/Dev/space-lens/.github/workflows/CI.yml#L23-L54)

## Branch & Commit Conventions

- CI runs on pushes to `main` and on pull requests.
- Renovate bot manages dependency updates with prefix `chore: bump up <package> version`.
- Version tags follow `v*` pattern for `space-lens` and `cli-v*` for `@space-lens/cli`.
- No PR template or specific branch naming convention is enforced.

**Section sources**

- [.github/workflows/CI.yml](file:///Users/hk/Dev/space-lens/.github/workflows/CI.yml#L7-L18)
- [.github/renovate.json](file:///Users/hk/Dev/space-lens/.github/renovate.json#L16-L18)

## Dependency Management

- Renovate runs with `:preserveSemverRanges` — semver ranges in `package.json` are not bumped to exact versions.
- `:disablePeerDependencies` — peer deps are not auto-updated.
- napi-rs dependencies are grouped under a single "napi-rs" Renovate group.

**Section sources**

- [.github/renovate.json](file:///Users/hk/Dev/space-lens/.github/renovate.json#L1-L20)
