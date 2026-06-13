# Space Lens — Agent Instructions

## Monorepo layout

| Path | Type | Published as | Tech |
|------|------|-------------|------|
| `packages/space-lens/` | Rust library | — | scanner + cleanup core |
| `packages/node/` | NAPI-RS binding | `space-lens` | napi-rs, Rust → Node.js |
| `apps/cli/` | Rust CLI | — | clap, uses `space-lens` crate |
| `apps/tui/` | Terminal TUI | `@space-lens/cli` | Effect, OpenTUI, TypeScript |

Workspace: Yarn 4 (`nodeLinker: node-modules`) + Cargo workspace (resolver 2).

## Development commands

```bash
yarn install                    # install everything
yarn build:debug                # build debug NAPI bindings (needed before local test)
yarn build:node                 # build release NAPI bindings
yarn build:tui                  # bundle TUI via tsdown
yarn test                       # cargo test + ava + node:test
yarn typecheck                  # run-p typecheck:node typecheck:tui
yarn lint                       # oxlint (node) + tsc --noEmit (tui)
yarn format                     # prettier + cargo fmt + taplo format
yarn bench                      # benchmark CLI (requires native binding)
yarn tui                        # bun apps/tui/src/cli.ts
cargo test --workspace          # Rust tests only
```

## Test runners

- **`space-lens` (packages/node)**: AVA. Tests are `.ts` files, transpiled by `@oxc-node/core/register`. Config in `package.json` under `"ava"`. Timeout 2m, `workerThreads: false`.
- **`@space-lens/cli` (apps/tui)**: Node.js built-in `node:test`. Run with `tsx`.
- **Rust**: `cargo test --workspace`.

Run focused: `yarn workspace space-lens test`, `yarn workspace @space-lens/cli test`.

## Key gotchas

1. **Build NAPI bindings first** — `yarn build:debug` before testing or benchmarking locally.
2. **Bun required for TUI** — OpenTUI 0.4.x needs Bun's native FFI. `yarn tui` always uses `bun`.
3. **AVA config is in `packages/node/package.json`** — includes `@oxc-node/core/register` for TS support (`--import` flag).
4. **oxlint, not eslint** — pre-commit runs `lint-staged` (oxlint --fix + prettier + taplo format). Config is in `packages/node/package.json`'s `"lint-staged"` key.
5. **Two TypeScript module configs** — `packages/node` uses `module: "Preserve"`/`"Bundler"`; `apps/tui` uses `"NodeNext"`/`"NodeNext"`.
6. **TUI bundler**: `tsdown`, not tsup or esbuild directly.
7. **Rustfmt**: 2-space tabs. `cargo fmt --all -- --check` in CI.
8. **Cleanup is dry-run by default** — the JS `planCleanup` returns a plan, `executeCleanup` actually deletes. Rust CLI needs `--execute`.
9. **Effect ecosystem** — TUI uses Effect CLI, Effect platform, Effect printer-ansi, and OpenTUI. Load the local Effect skill if available.
10. **Renovate** — semver ranges preserved (`:preserveSemverRanges`), peer deps disabled.
