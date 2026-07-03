# Getting Started Guide

<cite>
**Referenced Files in This Document**
- [README.md](file:///Users/hk/Dev/space-lens/README.md)
- [.yarnrc.yml](file:///Users/hk/Dev/space-lens/.yarnrc.yml)
- [package.json](file:///Users/hk/Dev/space-lens/package.json)
- [AGENTS.md](file:///Users/hk/Dev/space-lens/AGENTS.md)
- [packages/node/package.json](file:///Users/hk/Dev/space-lens/packages/node/package.json)
- [apps/tui/package.json](file:///Users/hk/Dev/space-lens/apps/tui/package.json)
</cite>

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Installation](#installation)
3. [Build Order](#build-order)
4. [Running the Applications](#running-the-applications)
5. [Running Tests](#running-tests)

## Prerequisites

| Tool | Version | Required For |
|------|---------|-------------|
| Node.js | >= 20 | Yarn, NAPI build, TUI typecheck |
| Rust toolchain | stable (latest) | Core library, bindings, CLI |
| Yarn | 4.14.1 (via Corepack) | JS package management |
| Bun | >= 1.3.0 | TUI runtime (OpenTUI FFI) |
| cargo-zigbuild | optional | musl target builds in CI |

Enable Corepack for Yarn 4:
```bash
corepack enable
```

**Section sources**

- [package.json](file:///Users/hk/Dev/space-lens/package.json#L41)
- [apps/tui/package.json](file:///Users/hk/Dev/space-lens/apps/tui/package.json#L14-L16)
- [AGENTS.md](file:///Users/hk/Dev/space-lens/AGENTS.md)

## Installation

```bash
git clone <repo-url>
cd space-lens
yarn install
```

Yarn 4 uses `nodeLinker: node-modules` — it installs into a flat `node_modules/` rather than using Plug'n'Play.

**Section sources**

- [.yarnrc.yml](file:///Users/hk/Dev/space-lens/.yarnrc.yml#L1)
- [package.json](file:///Users/hk/Dev/space-lens/package.json#L5-L7)

## Build Order

The NAPI native binding must be built before any JS tests or the TUI can use it:

```bash
# 1. Build debug NAPI binding (required before local testing)
yarn build:debug

# 2. Build release NAPI binding (for production use)
yarn build:node

# 3. Build TUI bundle
yarn build:tui
```

| Command | What it does |
|---------|-------------|
| `yarn build:debug` | Runs `napi build --platform` in packages/node — produces debug `.node` binary |
| `yarn build:node` | Runs `napi build --platform --release` — release binary |
| `yarn build:tui` | Runs `tsdown` in apps/tui — produces ESM bundle in `dist/` |

**Section sources**

- [packages/node/package.json](file:///Users/hk/Dev/space-lens/packages/node/package.json#L52-L53)
- [apps/tui/package.json](file:///Users/hk/Dev/space-lens/apps/tui/package.json#L30)
- [AGENTS.md](file:///Users/hk/Dev/space-lens/AGENTS.md)

## Running the Applications

**Rust CLI** (cargo):
```bash
cargo run -p space-lens-cli -- scan ~/Dev --json
cargo run -p space-lens-cli -- candidates ~/Dev --preset node
cargo run -p space-lens-cli -- clean ~/Dev --preset node --execute
```

**TUI** (requires Bun):
```bash
yarn tui ~/Dev --preset rust
yarn tui ~/Dev --preset node,gitignored --sort path
npx @space-lens/cli ~/Dev --preset rust
```

**Programmatic API** (Node.js):
```ts
import { scanDirectory } from 'space-lens'

const tree = scanDirectory({
  directories: [process.cwd()],
  ignoreHidden: false,
  fullPath: false,
  respectGitignore: true,
  ignoredMode: 'summarize',
})
```

**Section sources**

- [README.md](file:///Users/hk/Dev/space-lens/README.md#L13-L41)
- [README.md](file:///Users/hk/Dev/space-lens/README.md#L72-L93)

## Running Tests

```bash
# Run all tests (Rust + Node.js + TUI)
yarn test

# Run focused test suites
cargo test --workspace           # Rust only
yarn workspace space-lens test   # AVA (packages/node)
yarn workspace @space-lens/cli test  # node:test (apps/tui)

# Type checking
yarn typecheck                   # both workspaces
yarn workspace space-lens typecheck
yarn workspace @space-lens/cli typecheck

# Linting
yarn lint                        # oxlint + tsc --noEmit
cargo clippy --workspace --all-targets
cargo fmt --all -- --check
```

**Section sources**

- [package.json](file:///Users/hk/Dev/space-lens/package.json#L23-L28)
- [AGENTS.md](file:///Users/hk/Dev/space-lens/AGENTS.md)
