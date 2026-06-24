# Space Lens Kunkun Plugin Handoff — 2026-06-18

## Current State

Space Lens now has one shared Svelte UI and one `SpaceLensAPI` contract with transport-specific adapters:

- standalone browser/NPX mode: Hono static host plus token-gated `kkrpc/ws` at `/rpc`.
- Kunkun custom-view mode: `createKunkunClient()` talks to Kunkun host APIs and spawns `dist/backend.js`.
- demo mode: in-memory fake API.

The old privileged REST `/api/*` surface has been removed from standalone mode. The CLI prints a tokenized URL containing `spaceLensRpc=ws://.../rpc?token=...`.

The Kunkun plugin wrapper lives in `apps/kunkun-plugin`. Its backend wraps the shared `createSpaceLensAPI()` and enforces host-derived `KUNKUN_BACKEND_FS_READ_ROOTS` before scanner or cleanup planning calls. Native cleanup remains disabled in the Kunkun backend; the frontend adapter performs host confirmation and deletes through Kunkun `system.trash`.

## Important Fixes Already Applied

- `apps/kunkun-plugin/scripts/copy-web-assets.mjs` now filters copied `.node` files to Space Lens' declared NAPI artifacts. It should not copy unrelated native files from dev dependencies into `dist/node_modules/space-lens`.
- `apps/tui/src/scanner.ts` no longer falls back to synchronous `scanDirectory()` when `scanDirectoryWithProgress` is missing. It fails explicitly instead of blocking the backend event loop and causing kkrpc timeouts.
- `apps/tui/src/web-service.ts` schedules scans on the next task so `startScan()` can return a `ScanSession` before scanner work begins.
- `apps/kunkun-plugin/package.json` `build` and `build:backend` rebuild `@space-lens/cli` first, so backend bundles do not use stale `apps/tui/dist/*`.
- The custom-view header has Kunkun-only traffic-light padding and Electron drag regions in `ScanPicker.svelte` and `+page.svelte`.

## Known Issues / Follow-Ups

### 1. Kunkun Workspace/Submodule Wiring

Space Lens is now expected to live under the Kunkun repo as `extensions/space-lens` and to use pnpm workspaces. The plugin imports unpublished packages by package name:

```ts
import { exposeBackend } from '@kunkunsh/api/backend'
import { startHeadlessServer } from '@kunkunsh/headless'
```

The Kunkun root `pnpm-workspace.yaml` must keep `extensions/space-lens/apps/kunkun-plugin` in scope so `@kunkunsh/api` and `@kunkunsh/headless` resolve through workspace links until those packages are published.

### 2. Kunkun Host Changes Are in the Kunkun Worktree

The Space Lens plugin depends on local Kunkun host changes for:

- a simple `backend` permission for managed backend processes.
- host-derived backend read roots from effective scoped `fs-read` manifest entries plus dynamic grants.
- custom-view `system.trash` requiring `system-trash` plus scoped `fs-write`.
- `showInFinder` requiring `system-finder` plus scoped `fs-read`.
- `kunkun-ext://` privileged scheme registration before Electron app ready.

`spawnBackend()` no longer accepts caller-provided filesystem allowlists. The frontend should request `fs-read` dynamically for the selected root with a user-facing reason; Kunkun decides whether to remember the grant and passes approved roots to the backend as `KUNKUN_BACKEND_FS_READ_ROOTS`.

### 3. Native Artifact Matrix Is Incomplete

The current submodule checkout does not have `space-lens.darwin-arm64.node` built under `packages/node`, so `apps/kunkun-plugin/dist/scanner-runtime-report.json` correctly reports `"currentPlatform.bundled": false`. The copy script now filters `.node` files to Space Lens' declared NAPI artifacts and no longer mistakes unrelated dev dependency binaries, such as `@oxc-node/core`, for the scanner.

Build or download the current-platform NAPI artifact before expecting real scanner mode to work from packaged plugin dist. Release CI should run:

```sh
SPACE_LENS_REQUIRE_ALL_NATIVE_ARTIFACTS=1 node apps/kunkun-plugin/scripts/copy-web-assets.mjs
```

That strict gate currently fails until all configured NAPI target `.node` artifacts are available.

### 4. Dev Workflow Still Requires Backend Restart

When editing backend or scanner code:

```sh
pnpm --filter @space-lens/kunkun-plugin build:backend
```

Then close the Space Lens custom-view window or kill the existing Space Lens backend from Kunkun's process manager. Refreshing only the Vite frontend is not enough because the backend process keeps the old `dist/backend.js` loaded.

### 5. Kunkun Dev Mode Uses Two Processes

Recommended local dev flow:

```sh
pnpm --filter web dev --host 127.0.0.1 --port 5173
pnpm --filter @space-lens/kunkun-plugin build:backend
```

Then open the plugin in Kunkun using the manifest `devMain`:

```json
"devMain": "http://127.0.0.1:5173?spaceLensMode=kunkun"
```

The frontend is HMR-served by Vite; the backend is still spawned by Kunkun from `apps/kunkun-plugin/dist/backend.js`.

## Verification Snapshot

Most recent checks from the Kunkun submodule setup:

- `bun test tests/*.test.ts` in `apps/web`: 10 tests passed.
- `bun test tests/*.test.ts` in `apps/kunkun-plugin`: path-policy and packaging resolver tests passed, but browser/headless custom-view smokes need loopback server permission outside Codex's default sandbox, and the scanner runtime report correctly fails the current-platform bundled assertion until `space-lens.darwin-arm64.node` is built.
- `pnpm --filter @kunkunsh/api typecheck`, `pnpm --filter @kunkunsh/plugin-runtime typecheck`, and `pnpm --filter @kunkunsh/core typecheck`: passed from the Kunkun root.
- `pnpm --filter @kunkunsh/core test -- tests/backend-spawn-process.test.ts`: passed.
- `pnpm --filter kunkun-electron exec vitest run electron/__tests__/plugin-host-api-service.test.ts electron/__tests__/plugin-manager-space-lens.test.ts`: passed.

Manual verification from Kunkun:

- Vite custom-view UI loads in `spaceLensMode=kunkun`.
- Scan can run after killing stale backend process and respawning with the rebuilt scanner runtime.
