# Space Lens

Fast directory scanning utilities for Node.js, powered by Rust and napi-rs.

Space Lens exposes two scanner styles:

- `buildDirectoryTree`: returns the full tree from the dust-backed scanner.
- `scanCompact`: returns a smaller tree that can respect `.gitignore` and collapse ignored directories into summary nodes.

## Installation

```bash
npm install space-lens
```

## API Usage

```ts
import { buildDirectoryTree, getLargestNodes, scanCompact } from "space-lens";

const fullTree = buildDirectoryTree({
  directories: [process.cwd()],
  ignoreHidden: false,
  fullPath: false,
});

const largestNodes = getLargestNodes(fullTree, 10);
console.dir(largestNodes, { depth: null });

const compactTree = scanCompact({
  directories: [process.cwd()],
  ignoreHidden: false,
  fullPath: false,
  respectGitignore: true,
  ignoredMode: "summarize",
});

console.dir(compactTree, { depth: 3 });
```

## Compact Scanning

`scanCompact` is intended for large folders where keeping every file node in memory is too expensive. With `ignoredMode: "summarize"`, ignored directories such as `target/` or `node_modules/` are scanned for total size but returned as collapsed leaf nodes:

```ts
{
  name: "target",
  size: 1238249472,
  children: [],
  ignored: true,
  collapsed: true
}
```

Use `ignoredMode: "exclude"` to skip ignored paths entirely.

## Benchmark CLI

This repository includes a local CLI for comparing scanners and exporting trees:

```bash
yarn bench ~/Dev --mode compact
yarn bench ~/Dev --mode dust --top 20
yarn bench ~/Dev --mode both --no-json-size
yarn bench ~/Dev --mode compact --export-tree tree.json
```

Options:

```text
--mode both|dust|compact
--top N
--export-tree PATH
--json-size / --no-json-size
--ignore-hidden
--full-path
--respect-gitignore / --no-respect-gitignore
--ignored-mode summarize|exclude
```

In `compact` or `dust` mode, `--export-tree` writes the selected tree array. In `both` mode, it writes `{ dust, compact }`.

## Development

```bash
yarn install
yarn build:debug
yarn test
yarn typecheck
cargo check
cargo clippy --all-targets --all-features
```

Useful local commands:

- `yarn build`: build release bindings for the current platform.
- `yarn build:debug`: build debug bindings for local testing.
- `yarn test`: run AVA tests against the generated native binding.
- `yarn typecheck`: type-check the local TypeScript CLI.
- `yarn bench`: run the benchmark CLI through `bun cli.ts`.

## Publishing

```bash
npm version patch
git push --follow-tags
```
