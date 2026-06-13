# Space Lens Web GUI Specification

## Purpose

Build a DaisyDisk-like graphical frontend for Space Lens in `apps/web/`.

Space Lens already has a Rust scanner and a TypeScript TUI. The web GUI should reuse the existing scanning and cleanup backend while providing a richer visual experience: a radial disk-usage chart, breadcrumb navigation, drill-down exploration, cleanup planning, and deletion progress.

Canonical spec locations:

- `apps/web/SPEC.md` - app-local entrypoint for implementation loops.
- `docs/superpowers/specs/2026-06-13-space-lens-web-gui-design.md` - Superpowers design spec copy.

This spec is written for an AI implementation loop. Keep these constraints in memory on every iteration:

- The UI lives in `apps/web/`.
- The visual reference images live in `apps/web/references/`.
- The chart is a `sunburst` / `radial partition` visualization inspired by DaisyDisk.
- The frontend must be a static SPA that can run in a normal browser and later inside Kunkun as a plugin custom view.
- The SPA should use SvelteKit hash routing so direct navigation only needs the server to return the root HTML. Avoid depending on reverse-proxy rewrites to a fallback page.
- Do not send the whole filesystem tree to the browser when scanning large folders. The backend must page/slice the tree by visible depth and current path.
- Transport must be abstracted so the same UI can use WebSocket in standalone NPX mode and Kunkun backend/stdout-stdin RPC in plugin mode.

## Current Repository Context

Repository root:

- `/Users/hk/Dev/space-lens`

Important repo files:

- `AGENTS.md` - repository instructions, commands, monorepo layout, gotchas.
- `package.json` - root Yarn workspace config.
- `Cargo.toml` - Rust workspace config.
- `packages/space-lens/` - Rust scanner and cleanup core.
- `packages/node/` - NAPI-RS package published as `space-lens`.
- `packages/node/index.d.ts` - TypeScript types exported by the NAPI package.
- `apps/tui/` - existing terminal UI package published as `@space-lens/cli`.
- `apps/tui/src/scanner.ts` - current TypeScript wrapper around `scanDirectory`, `planCleanup`, and `executeCleanup`.
- `apps/tui/src/model.ts` - current TUI data types and formatting helpers.
- `apps/tui/src/cli.ts` - existing TUI CLI entrypoint.
- `apps/web/` - new empty SvelteKit web app for this project.

Current `apps/web` files:

- `apps/web/package.json` - SvelteKit 2 / Svelte 5 / Vite 8 / Tailwind 4 app.
- `apps/web/README.md` - currently minimal.
- `apps/web/vite.config.ts` - currently configures Tailwind and SvelteKit. It also currently passes `adapter-static` and Svelte runes options to `sveltekit(...)`.
- `apps/web/src/routes/+page.svelte` - currently the default welcome page; replace it with the app UI.
- `apps/web/src/routes/+layout.svelte` - imports `layout.css`, sets favicon, renders children.
- `apps/web/src/routes/layout.css` - currently only `@import 'tailwindcss';`.
- `apps/web/src/app.html` - SvelteKit HTML template.
- `apps/web/src/lib/index.ts` - empty placeholder for `$lib`.

Reference images:

- `apps/web/references/daisydisk1.png` - DaisyDisk detail screen with sunburst chart, breadcrumb path, right-side directory list, cleanup progress bar.
- `apps/web/references/daisydisk2.png` - DaisyDisk start screen with volume/folder list, scan buttons, usage bars.
- `apps/web/references/daisydisk3.png` - DaisyDisk detail screen showing right-click context menu actions and the bottom collector/delete workflow.

Kunkun sample plugin references outside this repo:

- `/Users/hk/Dev/kunkun/extensions/wterm-terminal-demo`
- `/Users/hk/Dev/kunkun/extensions/ai-config-manager`

Relevant Kunkun patterns from those samples:

- `wterm-terminal-demo` uses a Svelte/Vite custom view and a separate backend process.
- Frontend custom views import `@kunkunsh/api/ui/custom`.
- Frontend starts backend with `spawnBackend({ scriptPath: "$EXTENSION/dist/backend.js", runtime: "node" })`.
- Backend exposes API with `exposeBackend()` from `@kunkunsh/api/backend`.
- Kunkun frontend-to-backend communication is KKRPC over a backend process relay; the backend side uses stdio.
- `ai-config-manager` already has a runtime mode pattern that switches between Kunkun, WebSocket, and demo modes.

## Existing Backend API

The NAPI package `space-lens` currently exposes these key functions in `packages/node/index.d.ts`:

- `scanDirectory(options: DirectoryScanOptions): DirectoryNode[]`
- `findCleanupCandidates(options: CleanupCandidateOptions): CleanupCandidate[]`
- `planCleanup(options: CleanupCandidateOptions): RemovalPlan`
- `executeCleanup(plan: RemovalPlan): RemovalOutcome`

Important current types:

```ts
export interface DirectoryNode {
  name: string
  path: string
  size: number
  children: Array<DirectoryNode>
  depth: number
  ignored: boolean
  collapsed: boolean
}

export interface DirectoryScanOptions {
  directories: Array<string>
  ignoreHidden?: boolean
  fullPath?: boolean
  respectGitignore?: boolean
  ignoredMode?: string
}

export interface RemovalEntry {
  path: string
  size: number
  reason: string
  preset: string
}

export interface RemovalPlan {
  entries: Array<RemovalEntry>
  totalSize: number
  errors: Array<string>
}
```

The existing TUI wrapper in `apps/tui/src/scanner.ts` calls:

- `scanDirectory({ directories, ignoreHidden, fullPath: false, respectGitignore: true, ignoredMode: 'summarize' })`
- `planCleanup({ directories, presets, ignoreHidden })`
- `executeCleanup(plan)`

The first GUI implementation may wrap these synchronous APIs, but the public GUI RPC contract should be designed so it can support streaming progress and lazy tree loading later.

## Product Requirements

### Start Screen

The app should open to a scan target selection screen similar to `apps/web/references/daisydisk2.png`.

Required behavior:

- Show a compact app window surface, not a marketing landing page.
- Let the user scan a whole disk/volume when the host can list volumes.
- Let the user scan one or more specific folders. This is a core Space Lens requirement, not a fallback.
- In browser/standalone mode, support entering or choosing a path depending on what the host can provide.
- In Kunkun plugin mode, use Kunkun host APIs for native directory picking if available.
- Show known scan targets or recent paths when available.
- Show scan status and errors clearly.

MVP acceptable behavior:

- A path text input plus `Scan` button is acceptable for standalone mode.
- A multi-path text input or repeatable folder row is acceptable for the first folder-only implementation.
- A mock/recent list is acceptable only before backend integration. Remove fake data from production flow once the backend exists.

### Scan Scope

Do not assume the product only scans entire disks.

Supported scan roots:

- Whole disk or mounted volume, when the host can enumerate volumes.
- A single user-selected folder.
- Multiple user-selected folders in one scan session.

Behavior:

- If scanning multiple folders, treat them as multiple top-level roots under a synthetic parent such as `Selected Folders`.
- The breadcrumb should make it clear whether the current root is a disk, a mounted volume, or a user-selected folder.
- Folder scans should not require administrator permissions or disk-level APIs.
- Recent scan targets should remember both disks and folders.
- The backend API already accepts `directories: string[]`; preserve this capability in the GUI API.
- Kunkun plugin mode should prefer native directory selection through host APIs when available.
- Browser/standalone mode may use a path text field first, because normal browsers cannot freely choose arbitrary filesystem paths without a host bridge.

### Detail Screen

The main screen should be inspired by `apps/web/references/daisydisk1.png`.

Required regions:

- Top breadcrumb path, e.g. `Disks and Folders > Macintosh HD > Users > hk > Dev`.
- Left/main radial sunburst chart.
- Center label showing the current focused directory size.
- Right-side sorted child list with color dots, names, and sizes.
- Bottom status/progress area for scanning or deletion.
- Stop/cancel control for long operations when supported.

Required interactions:

- Hover an arc: highlight matching list row and show path/size.
- Hover a list row: highlight matching arc.
- Click an arc/list row: drill into that node.
- Right-click or context-menu a list row/arc to show node actions inspired by `apps/web/references/daisydisk3.png`.
- Use breadcrumb segments to navigate back up.
- Keep the current path and loaded node state in frontend state.
- Keep UI responsive during loading and large scans.

Context menu actions:

- Expand/drill into the selected folder.
- Preview if a preview mechanism exists.
- Show in Finder on macOS/Kunkun when host APIs permit it.
- Open in Terminal when host APIs permit it.
- Move the selected node to Collector.

MVP context-menu behavior:

- `Expand` and `Move to Collector` should work in the demo UI.
- `Preview`, `Show in Finder`, and `Open in Terminal` can be disabled or hidden until host APIs are wired.

### Visual Style

Use the screenshots as inspiration, not a pixel-perfect clone.

Visual intent:

- Dark desktop-app surface.
- Dense, practical information layout.
- Sunburst uses green/yellow/orange colors similar to DaisyDisk.
- Avoid a generic dashboard card layout.
- Avoid a marketing hero.
- Avoid oversized explanatory text in the app.
- Use icon buttons where useful, especially for back, forward, refresh, stop, open folder, settings.

The chart type is:

- Common name: `sunburst chart`
- Layout name: `radial partition`
- Recommended implementation: D3 `hierarchy` + `partition` + `arc`

## Architecture Requirements

### High-Level Target Architecture

Use one frontend and multiple transport adapters:

```text
apps/web
  Static SvelteKit SPA
  DaisyDisk-like UI
  Uses SpaceLensClient interface only

shared contract package or app-local lib
  SpaceLensAPI TypeScript interface
  DTO types
  client creation helpers

standalone NPX/server mode
  serves static apps/web build output
  exposes SpaceLensAPI over KKRPC WebSocket
  browser connects with kkrpc/ws

Kunkun plugin mode
  Kunkun custom view loads same SPA
  frontend calls spawnBackend()
  backend exposes SpaceLensAPI with exposeBackend()
  transport is Kunkun backend relay/stdin-stdout via KKRPC

backend implementation
  wraps the existing space-lens NAPI package
  owns scan sessions, lazy tree slices, cleanup planning, cleanup execution
```

### Recommended Package/File Layout

Prefer this layout unless there is a strong reason to change it:

```text
apps/web/
  SPEC.md
  src/
    lib/
      api/
        types.ts              # SpaceLensAPI, DTOs, errors
        client.ts             # createSpaceLensClient()
        browser-mode.ts       # detect demo/websocket/kunkun mode
        websocket-client.ts   # kkrpc/ws client adapter
        kunkun-client.ts      # spawnBackend adapter, only loaded in Kunkun mode
        demo-client.ts        # optional dev-only sample data adapter
      chart/
        sunburst.ts           # D3 layout helpers, pure functions where possible
        colors.ts             # stable color strategy for nodes
      components/
        SunburstChart.svelte
        BreadcrumbBar.svelte
        ChildList.svelte
        ScanPicker.svelte
        StatusBar.svelte
      state/
        explorer.svelte.ts    # Svelte 5 runes state for current scan/path/cache
    routes/
      +layout.svelte
      +page.svelte
      layout.css
```

If shared packages are added later, use:

```text
packages/gui-contract/
  src/index.ts

packages/gui-backend/
  src/index.ts
```

But for a first implementation loop, keeping the contract under `apps/web/src/lib/api/types.ts` is acceptable as long as it is clean and easy to move later.

### Static SPA Requirement

The web frontend should build as static assets.

Requirements:

- Use SvelteKit as an SPA/static app.
- Use SvelteKit hash routing: configure `kit.router.type = 'hash'`.
- Keep `kit.router.resolution = 'client'` when hash routing is enabled.
- Do not add SvelteKit server-only files such as `+server.ts`, `+page.server.ts`, or `+layout.server.ts` under `apps/web/src/routes/`; SvelteKit rejects server-only route files with `router.type === 'hash'`.
- Do not require a reverse proxy, `proxy_pass`, `.htaccess`, or a static-host fallback rewrite for deep app navigation.
- App navigation state should live in `/#/...` or frontend state/query/hash values so normal static hosting and Kunkun custom-view loading do not produce 404s.
- Do not rely on SvelteKit server routes for the core app.
- Do not put scanning logic in SvelteKit `+server.ts` routes.
- The standalone host can serve the static build output and expose a separate RPC endpoint.
- Kunkun plugin mode should be able to load the same built SPA as a custom view.

Implementation note:

- Current `apps/web/vite.config.ts` already uses `@sveltejs/adapter-static`.
- SvelteKit 2.62+ allows config to be passed through the `sveltekit(...)` Vite plugin. This app currently uses that style in `apps/web/vite.config.ts`.
- If implementation needs a conventional SvelteKit setup, it may add `apps/web/svelte.config.js` and adjust config carefully. Do not break the existing Svelte 5 runes setup.
- If both config styles exist, remember SvelteKit ignores `svelte.config.js` when config is passed directly to the Vite plugin.

### RPC and Transport Requirements

Use KKRPC as the primary RPC abstraction.

Recommended API import strategy:

- Use default `kkrpc` for normal request/response methods.
- Use `kkrpc/streaming` only if returning `AsyncIterable` from scan/progress APIs.
- Use `kkrpc/ws` for standalone browser-to-server mode.
- Use Kunkun `spawnBackend()` and `@kunkunsh/api/backend` `exposeBackend()` for plugin mode.

The frontend must call a stable `SpaceLensAPI` interface. The UI must not directly know whether it is using WebSocket, Kunkun stdio backend, or demo data.

Potential runtime mode detection:

- Kunkun mode if Electron/Kunkun IPC is present.
- WebSocket mode if URL has something like `?spaceLensRpc=auto` or `?spaceLensRpc=ws://127.0.0.1:PORT/rpc`.
- Demo mode only for local UI development when no backend exists.

### Proposed `SpaceLensAPI`

Design the API around scan sessions and lazy tree loading:

```ts
export interface SpaceLensAPI {
  startScan(options: StartScanOptions): Promise<ScanSession>
  getNode(request: GetNodeRequest): Promise<TreeSlice>
  getChildren(request: GetChildrenRequest): Promise<ChildrenPage>
  getScanStatus(scanId: string): Promise<ScanStatus>
  cancelScan(scanId: string): Promise<void>
  planCleanup(options: CleanupPlanOptions): Promise<RemovalPlan>
  executeCleanup(options: ExecuteCleanupOptions): Promise<CleanupOutcome>
  showInFileManager?(path: string): Promise<void>
  openInTerminal?(path: string): Promise<void>
}
```

Potential streaming version:

```ts
export interface SpaceLensStreamingAPI {
  scan(options: StartScanOptions): AsyncIterable<ScanEvent>
}
```

Use the non-streaming session API first if it is simpler. Keep the types compatible with a future streaming implementation.

Suggested DTOs:

```ts
export interface StartScanOptions {
  paths: string[]
  ignoreHidden: boolean
  respectGitignore: boolean
  ignoredMode: 'summarize' | 'exclude'
  initialDepth: number
  maxChildrenPerNode: number
}

export interface ScanSession {
  scanId: string
  rootIds: string[]
  createdAt: string
}

export interface TreeNodeSummary {
  id: string
  name: string
  path: string
  size: number
  depth: number
  ignored: boolean
  hasChildren: boolean
  childCount?: number
  loadedDepth: number
  truncated: boolean
}

export interface TreeSlice {
  scanId: string
  focusNode: TreeNodeSummary
  ancestors: TreeNodeSummary[]
  children: TreeNodeSummary[]
  totalSize: number
  loadedDepth: number
  maxDepth: number
  truncated: boolean
  omittedBytes: number
  omittedCount: number
}

export interface GetNodeRequest {
  scanId: string
  nodeId: string
  depth: number
  maxChildrenPerNode: number
}

export interface GetChildrenRequest {
  scanId: string
  nodeId: string
  offset: number
  limit: number
  sort: 'size' | 'name'
}
```

Exact names can change, but the API must preserve the lazy-loading model.

Collector state may live in the frontend at first, but define types that can be promoted into the backend later:

```ts
export interface CollectorEntry {
  id: string
  scanId: string
  nodeId: string
  path: string
  name: string
  size: number
  addedAt: string
}

export interface CollectorState {
  entries: CollectorEntry[]
  totalSize: number
}
```

## Critical Performance Requirements

This is the most important non-visual requirement.

The backend may scan huge directory trees. It is not acceptable to send tens or hundreds of megabytes of tree JSON to the browser.

Rules:

- Never send the full scan tree to the frontend by default.
- Frontend requests only the currently visible slice.
- The default visible depth should be about 3 levels.
- When the user drills into a child node, request that child node's slice from the backend.
- Each node slice should cap the number of children returned.
- Children should be sorted by size descending by default.
- Small children may be aggregated into an `Other` or `omitted` bucket.
- The frontend should maintain only a bounded cache of loaded slices.
- The backend should own the authoritative scan session and tree index.
- The API should support cancellation for scans and cleanup when backend support exists.

Suggested defaults:

- `initialDepth`: 3
- `maxChildrenPerNode`: 200 for chart data
- `rightPanelPageSize`: 100
- `frontendSliceCacheLimit`: 30 slices
- Avoid sending responses larger than roughly 1-2 MB during normal navigation.

Implementation strategy:

- Backend scans once and indexes tree nodes by stable `nodeId`.
- Backend stores parent/child relationships in memory for the session.
- Frontend asks `getNode({ scanId, nodeId, depth: 3, maxChildrenPerNode: 200 })`.
- Backend returns only descendants within that depth.
- Frontend asks `getChildren({ scanId, nodeId, offset, limit })` for the right-side list pagination.
- If current Rust/NAPI API only returns a full tree, the first implementation may scan into backend memory, but must still slice before sending data to the browser.

## Sunburst Data Requirements

The chart should receive a bounded hierarchy for the current focus node.

Chart node requirements:

- `id`
- `name`
- `path`
- `size`
- `children`
- `ignored`
- `truncated`
- optional `omittedBytes`

Behavior:

- The focus node appears as the chart center.
- Descendants appear as radial rings.
- Ring count should correspond to loaded depth.
- The chart should not render unbounded thousands of arcs.
- If data is truncated, show a visual indication and make it clear more data exists.
- Color should be stable for a node while navigating.

Use D3 for layout:

- `d3.hierarchy`
- `d3.partition`
- `d3.arc`

Prefer pure helper functions for layout calculations so they can be unit tested without Svelte.

## Cleanup Requirements

The existing product has cleanup planning and execution.

The DaisyDisk-inspired cleanup model is a collector:

- Users do not have to delete immediately after seeing a large folder.
- Users can move folders/files into a Collector from the context menu.
- The bottom status area shows the total collected size and a prominent delete action.
- Users can open the Collector list before deletion.
- Users can remove/unselect entries from the Collector before deletion.
- Actual deletion happens only after explicit confirmation.
- Collector entries should survive navigation within the current scan session.
- Collector entries should be validated before deletion, because files may move or disappear after scanning.

Rules:

- Cleanup must be dry-run/planning first.
- Do not delete without explicit confirmation.
- Show selected cleanup entries and total bytes to remove.
- Prevent duplicate collector entries for the same path/node.
- If a parent folder is in the Collector, adding a child should either be blocked or clearly de-duplicated so bytes are not double-counted.
- If a child is in the Collector and the user adds its parent, replace the child with the parent or ask for confirmation.
- Show errors returned by `planCleanup` and `executeCleanup`.
- Deletion progress/status should appear in the bottom status area similar to the DaisyDisk reference.
- In Kunkun plugin mode, be careful with permissions and host sandboxing.

MVP:

- It is acceptable to expose scan/explore first and make Collector use demo/local state before backend deletion is wired.
- If cleanup is deferred, the UI should not pretend deletion works.
- A non-destructive Collector with total size and remove/unselect behavior is part of the desired first UI loop.

## Error Handling Requirements

Handle:

- Backend unavailable.
- WebSocket connection failure.
- Kunkun backend spawn failure.
- Invalid path.
- Permission denied.
- Scan cancelled.
- Scan failed.
- Cleanup failed partially.
- Very large directory where data is truncated.

Errors should be visible in the UI and actionable. Do not hide them in the console only.

## Accessibility and Usability

Requirements:

- Keyboard reachable controls for scan, back, breadcrumb, list navigation, and stop/cancel.
- Right-side list should be readable and scrollable.
- Text must not overlap or overflow its container at common desktop sizes.
- The UI should support resizable windows.
- Use semantic buttons and labels for controls.
- Chart hover interactions should have list equivalents for users who cannot precisely use the pointer.

## Development Requirements

Follow repo instructions in `AGENTS.md`.

Relevant commands:

```bash
yarn install
yarn build:debug
yarn workspace web check
yarn workspace web build
```

If `apps/web` is not in the root Yarn workspace yet, add it to the root `package.json` workspaces before relying on `yarn workspace web ...`.

Before using or changing library-specific APIs, follow `AGENTS.md` and use `ctx7` for current documentation. This matters for SvelteKit, Svelte, D3, KKRPC, and Kunkun APIs.

For frontend implementation, also use the official Svelte MCP/docs if available in the coding environment.

## Testing Requirements

Minimum expected verification:

- `yarn workspace web check`
- `yarn workspace web build`

When chart helper logic exists:

- Add unit tests for slicing/layout helpers if a test runner is configured.
- Test that `getNode`/slice logic does not return full deep trees.
- Test that child count and omitted bytes are preserved.

When browser verification is available:

- Run the dev server.
- Open the app in browser automation.
- Verify the sunburst is visible and non-empty.
- Verify drill-down and breadcrumb navigation.
- Verify the app does not freeze on large mock data.

## Milestones

### Milestone 1: Static UI and Mock Data

- Replace default `+page.svelte`.
- Build DaisyDisk-like layout.
- Add bounded mock tree data.
- Render sunburst chart from mock `TreeSlice`.
- Implement breadcrumb and right-list drill-down against mock data.
- Support mock scan roots for both a whole volume and one or more selected folders.
- Add context menu affordance for child rows/arcs.
- Add non-destructive Collector state, bottom collected-size display, and Collector list/remove flow.
- Keep all UI code in `apps/web`.

### Milestone 2: API Contract and Client Abstraction

- Add `SpaceLensAPI` and DTO types.
- Add `createSpaceLensClient()` runtime mode selection.
- Add demo client and WebSocket client stub.
- Ensure UI only talks to the API abstraction.

### Milestone 3: Standalone Backend

- Add server/CLI package or script that serves `apps/web` static output.
- Expose `SpaceLensAPI` over KKRPC WebSocket.
- Wrap `space-lens` NAPI scanning.
- Slice full backend scan data before sending to browser.

### Milestone 4: Kunkun Plugin Mode

- Add Kunkun extension packaging.
- Reuse same built SPA.
- Add backend process using `exposeBackend()`.
- Add frontend Kunkun client using `spawnBackend()`.
- Verify it follows patterns from:
  - `/Users/hk/Dev/kunkun/extensions/wterm-terminal-demo`
  - `/Users/hk/Dev/kunkun/extensions/ai-config-manager`

### Milestone 5: Cleanup Workflow

- Plan cleanup candidates from Collector entries.
- Let user move entries into Collector from chart/list context menus.
- Let user open Collector and remove/unselect entries before deletion.
- Prevent duplicate and parent/child double-counting in Collector.
- Confirm before executing.
- Execute cleanup and show progress/errors.

### Milestone 6: Incremental/Streaming Scanner

- If needed, extend Rust/NAPI backend to emit progress and support cancellation.
- Upgrade RPC to streaming or callback events.
- Keep the lazy tree slice contract.

## Non-Goals for the First Loop

- Pixel-perfect DaisyDisk clone.
- SwiftUI native app.
- Full Kunkun plugin packaging if the standalone web UI is not working yet.
- Sending the entire tree to the browser.
- Deleting files without a confirmation flow.
- Building a marketing landing page.

## Acceptance Criteria

The implementation is on the right track when:

- `apps/web` shows a desktop-app-style Space Lens UI, not the default Svelte page.
- The main chart is a sunburst/radial partition.
- The UI uses `apps/web/references/daisydisk1.png` and `apps/web/references/daisydisk2.png` as visual references.
- The UI uses `apps/web/references/daisydisk3.png` for context-menu and Collector/delete workflow references.
- The scan flow supports specific folder roots, not only whole-disk scans.
- The frontend has an API abstraction for backend calls.
- The data model supports lazy loading by `scanId`, `nodeId`, `depth`, and child pagination.
- The backend or mock backend never sends an unbounded full tree to the UI.
- The UI includes a non-destructive Collector concept where entries can be added, reviewed, removed, and only then deleted after confirmation.
- SvelteKit builds to static assets.
- The design can later run in Kunkun using `spawnBackend()` without rewriting the UI.

## Target Prompt for Implementation Loop

Use this prompt to start an AI coding loop:

```text
You are working in /Users/hk/Dev/space-lens.

Read and follow:
- AGENTS.md
- apps/web/SPEC.md
- docs/superpowers/specs/2026-06-13-space-lens-web-gui-design.md

Primary goal:
Implement the Space Lens web GUI in apps/web as a static SvelteKit/Svelte 5 SPA inspired by the DaisyDisk screenshots in apps/web/references/daisydisk1.png, apps/web/references/daisydisk2.png, and apps/web/references/daisydisk3.png.

Hard constraints:
- Do not build a landing page. Build the actual app UI as the first screen.
- The chart must be a sunburst/radial partition visualization.
- The scan flow must support user-specified folder roots, not only whole-disk scans.
- The UI must include a DaisyDisk-like Collector concept: add items from context menu, show collected total, open/review Collector, remove/unselect entries, and require confirmation before deletion.
- The frontend must use a SpaceLensAPI/client abstraction so it can later switch between demo, WebSocket KKRPC, and Kunkun spawnBackend transports.
- Do not send or render a full huge filesystem tree in the frontend. Design and implement bounded TreeSlice data with visible depth, max children, omitted counts/bytes, and drill-down loading.
- Keep the app static-SPA compatible. Use SvelteKit hash routing (`kit.router.type = 'hash'`) so direct/deep app navigation does not require a reverse-proxy rewrite to a fallback page.
- Do not rely on SvelteKit server routes for scanning.
- Do not add server-only SvelteKit route files in apps/web when hash routing is enabled.
- Use paths and repo context exactly as described in apps/web/SPEC.md.

Suggested first loop:
1. Inspect apps/web and the reference images.
2. Add app-local API types for StartScanOptions, ScanSession, TreeNodeSummary, TreeSlice, SpaceLensAPI.
3. Add a demo client with bounded mock scan data and lazy getNode/getChildren behavior.
4. Replace apps/web/src/routes/+page.svelte with the real app UI.
5. Implement D3-based SunburstChart.svelte and supporting layout helpers.
6. Implement breadcrumb, child list, scan picker, context menu, Collector, and status bar.
7. Verify with yarn workspace web check and yarn workspace web build. If apps/web is not yet in the root workspace, fix the workspace config first.

Before using current library APIs for SvelteKit, Svelte, D3, KKRPC, or Kunkun, follow AGENTS.md and fetch current docs with ctx7. Use official Svelte docs/MCP when available.

Report what changed, which commands passed, and any remaining gaps.
```
