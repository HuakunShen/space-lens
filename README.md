# Space Lens

Fast directory scanning utilities for Node.js, powered by Rust and napi-rs.

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

## Directory Scanning

`scanDirectory` is intended for large folders where keeping every file node in memory is too expensive.
With `ignoredMode: 'summarize'`, ignored directories such as `target/` or `node_modules/` are scanned for total size but returned as collapsed leaf nodes:

```ts
{
  name: 'target',
  size: 1238249472,
  children: [],
  ignored: true,
  collapsed: true
}
```

Use `ignoredMode: 'exclude'` to skip ignored paths entirely.

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
yarn build:debug
yarn test
yarn typecheck
cargo check
```

Useful local commands:

- `yarn build`: build release bindings for the current platform.
- `yarn build:debug`: build debug bindings for local testing.
- `yarn test`: run AVA tests against the generated native binding.
- `yarn typecheck`: type-check the local TypeScript CLI.
- `yarn bench`: run the benchmark CLI.
