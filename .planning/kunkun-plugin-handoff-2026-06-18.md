# Space Lens Kunkun Plugin Handoff — 2026-06-18

## Current State

Space Lens now has one shared Svelte UI and one `SpaceLensAPI` contract with transport-specific adapters:

- standalone browser/NPX mode: Hono static host plus token-gated `kkrpc/ws` at `/rpc`.
- Kunkun custom-view mode: `createKunkunClient()` talks to Kunkun host APIs and spawns `dist/backend.js`.
- demo mode: in-memory fake API.

The old privileged REST `/api/*` surface has been removed from standalone mode. The CLI prints a tokenized URL containing `spaceLensRpc=ws://.../rpc?token=...`.

The Kunkun plugin wrapper lives in `apps/kunkun-plugin`. Its backend wraps the shared `createSpaceLensAPI()` and enforces `KUNKUN_BACKEND_FS_READ_ALLOW` before scanner or cleanup planning calls. Native cleanup remains disabled in the Kunkun backend; the frontend adapter performs host confirmation and deletes through Kunkun `system.trash`.

## Important Fixes Already Applied

- `apps/kunkun-plugin/scripts/copy-web-assets.mjs` now copies package artifacts first, then lets the root `packages/node/*.node` artifact override current-platform artifacts. This fixed the bug where `dist/node_modules/space-lens/space-lens.darwin-arm64.node` came from the older `packages/node/npm/darwin-arm64` artifact and exported `scanDirectoryWithProgress: undefined`.
- `apps/tui/src/scanner.ts` no longer falls back to synchronous `scanDirectory()` when `scanDirectoryWithProgress` is missing. It fails explicitly instead of blocking the backend event loop and causing kkrpc timeouts.
- `apps/tui/src/web-service.ts` schedules scans on the next task so `startScan()` can return a `ScanSession` before scanner work begins.
- `apps/kunkun-plugin/package.json` `build` and `build:backend` rebuild `@space-lens/cli` first, so backend bundles do not use stale `apps/tui/dist/*`.
- The custom-view header has Kunkun-only traffic-light padding and Electron drag regions in `ScanPicker.svelte` and `+page.svelte`.

## Known Issues / Follow-Ups

### 1. Cross-Repo Kunkun Imports Are Temporary

The plugin currently imports unpublished Kunkun source directly:

```ts
import { exposeBackend } from '../../../../kunkun/packages/api/src/backend/index.ts'
```

Tests also import Kunkun headless source directly:

```ts
import { startHeadlessServer } from '../../../../kunkun/packages/headless/src/index.ts'
```

This is acceptable only for local integration while `@kunkunsh/api` and `@kunkunsh/headless` are unpublished. The intended long-term shape is:

- add Space Lens into the Kunkun repo as a submodule or workspace member, or
- publish/version the required Kunkun packages, then depend on normal package names.

The immediate next attempt is to convert Space Lens from Yarn workspaces to pnpm, so it can be linked more naturally with Kunkun's pnpm workspace.

### 2. Kunkun Host Changes Are in the Kunkun Worktree, Not This Commit

The Space Lens plugin depends on local Kunkun host changes for:

- `BackendSpawnOptions.fsReadAllow`.
- host-validated backend read roots.
- custom-view `system.trash` requiring `system-trash` plus scoped `fs-write`.
- `showInFinder` requiring `system-finder` plus scoped `fs-read`.
- `kunkun-ext://` privileged scheme registration before Electron app ready.

Those changes are in `/Users/hk/Dev/kunkun`, mixed with other unrelated worktree changes. They are intentionally not committed from this Space Lens commit.

### 3. Native Artifact Matrix Is Incomplete

Local dev currently has the macOS arm64 native artifact. The plugin build writes `dist/scanner-runtime-report.json`; release CI should run:

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

Recommended local dev flow after package-manager migration:

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

Passed after the latest scanner/runtime fixes:

- `yarn workspace @space-lens/cli test` — 22 tests passed.
- `yarn workspace @space-lens/cli typecheck`.
- `yarn workspace @space-lens/kunkun-plugin test` — 8 tests passed.
- `yarn workspace @space-lens/kunkun-plugin check`.
- `yarn workspace @space-lens/kunkun-plugin build`.
- `yarn workspace web check`.
- `git diff --check`.

Manual verification from Kunkun:

- Vite custom-view UI loads in `spaceLensMode=kunkun`.
- Scan can run after killing stale backend process and respawning with the rebuilt scanner runtime.

