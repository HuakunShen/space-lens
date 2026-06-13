# Space Lens

Fast directory scanning and cleanup candidate utilities, powered by Rust and napi-rs.

## Installation

```bash
npm install space-lens
```

## API Usage

```ts
import { scanDirectory } from 'space-lens'

const tree = scanDirectory({
  directories: [process.cwd()],
  ignoreHidden: false,
  fullPath: false,
  respectGitignore: true,
  ignoredMode: 'summarize',
})

console.dir(tree, { depth: 3 })
```

Find cleanup candidates without deleting anything:

```ts
import { findCleanupCandidates, planCleanup } from 'space-lens'

const candidates = findCleanupCandidates({
  directories: [process.cwd()],
  presets: ['node', 'rust', 'gitignored'],
})

const plan = planCleanup({
  directories: [process.cwd()],
  presets: ['node'],
})
```

## Directory Scanning

`scanDirectory` is intended for large folders where keeping every file node in memory is too expensive.
With `ignoredMode: 'summarize'`, ignored directories such as `target/` or `node_modules/` are scanned for total size but returned as collapsed leaf nodes:

```ts
{
  name: 'target',
  path: '/path/to/project/target',
  size: 1238249472,
  children: [],
  ignored: true,
  collapsed: true
}
```

Use `ignoredMode: 'exclude'` to skip ignored paths entirely.

## Cleanup Candidates

Cleanup APIs are dry-run oriented. `findCleanupCandidates` reports matching paths and sizes, and `planCleanup` returns a removal plan. The npm package does not execute deletion.

Initial presets:

- `node`: reports `node_modules`.
- `rust`: reports Cargo `target` directories.
- `gitignored`: reports paths matched by `.gitignore`.

## Rust CLI

The workspace includes a simple Rust CLI app:

```bash
cargo run -p space-lens-cli -- scan ~/Dev --json
cargo run -p space-lens-cli -- candidates ~/Dev --preset node
cargo run -p space-lens-cli -- clean ~/Dev --preset node
```

`clean` defaults to dry-run. Add `--execute` only when you want to remove the planned paths.

## spacelens TUI CLI

The workspace also includes `spacelens`, an OpenTUI app with two modes: `scan` for a disk usage tree and `clean` for selecting cleanup candidates and deleting them after confirmation. OpenTUI 0.4.x uses native FFI that currently needs Bun at runtime:

```bash
yarn tui ~/Dev --preset rust
yarn tui ~/Dev --preset node,gitignored --sort path
npx spacelens ~/Dev --preset rust
```

Inside the TUI, press `tab` to switch modes, `space` to select a cleanup candidate, `x` to request deletion, and `enter` to confirm. Use `Ctrl+C` or `q` to quit.

## Benchmark CLI

This repository includes a local CLI for benchmarking the directory scanner and exporting trees:

```bash
yarn bench ~/Dev
yarn bench ~/Dev --no-json-size
yarn bench ~/Dev --export-tree tree.json
```

Options:

```text
--export-tree PATH
--json-size / --no-json-size
--ignore-hidden
--full-path
--respect-gitignore / --no-respect-gitignore
--ignored-mode summarize|exclude
```

## Development

```bash
yarn install
yarn workspace space-lens build:debug
yarn workspace spacelens build
yarn workspace space-lens test
yarn workspace spacelens test
yarn workspace space-lens typecheck
cargo test --workspace
```

Useful local commands:

- `yarn build`: build release bindings for the current platform.
- `yarn build:debug`: build debug bindings for local testing.
- `yarn tui`: run the Bun/OpenTUI cleanup candidate viewer.
- `yarn test`: run Rust workspace tests and AVA tests.
- `yarn typecheck`: type-check the TypeScript workspaces.
- `yarn bench`: run the benchmark CLI from the `space-lens` npm workspace.
