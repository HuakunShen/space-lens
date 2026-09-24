# NPM Package

**Updated: 2026-09-23** — `space-lens` @ 0.2.8 as of `5d74b29`.

## Package (`packages/node`, name `space-lens`)

- **napi-rs** bindings: `napi.binaryName: "space-lens"`, build via
  `yarn workspace space-lens build` (release) / `build:debug`.
- **8 binary targets**: x64/arm64 × windows-msvc, apple-darwin, linux-gnu,
  linux-musl — published as optionalDependencies
  (`space-lens-darwin-arm64` … `space-lens-win32-arm64-msvc`, all pinned to
  the package version).
- Entry: `index.js` / `index.d.ts`; bin: `bin/cli.mjs` (runs the TUI and
  `spacelens serve`); `files`: `bin`, `cli.ts`, `index.d.ts`,
  `index.js`, `scripts`.

## API surface

```ts
import { scanDirectory, findCleanupCandidates, planCleanup } from 'space-lens'
```

- `scanDirectory({ directories, ignoreHidden, fullPath, respectGitignore,
  ignoredMode })` — `ignoredMode: 'summarize'` returns collapsed sized leaves
  (`{ ignored: true, collapsed: true }`); `'exclude'` drops them.
- `findCleanupCandidates` / `planCleanup` — **dry-run only; the npm package
  never executes deletion**. Presets: `node`, `rust`, `gitignored`.
- `ICloudSession` — opaque one-shot plan IDs, decimal-string byte counts
  (`src/cloud.rs`).

## Consumers of the binding

- `apps/tui` scanner (`src/scanner.ts`)
- `packages/host` scan sessions (`src/scan-store.ts`, worker threads)
- External npm users (`npm install space-lens`)

## Tooling

AVA tests (`yarn workspace space-lens test`), oxlint, tsc --noEmit, taplo,
prettier; `prepublishOnly` builds the TUI, runs `prepare-cli.mjs`, then
`napi prepublish -t npm`. `@tauri-apps/api` is lockstep-pinned for the
desktop flavor transport.
