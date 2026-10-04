# Space Lens web workbench

The Svelte workbench runs in a browser or in the Tauri app. Shared components live in `packages/web-ui`; both hosts expose the same discovery and cleanup contract.

Previews: [Browse](preview/browse-web-dark.png) · [Developer cleanup](preview/developer-cleanup-web-dark.png) · [macOS treatment](preview/developer-cleanup-macos-light.png) · [Settings](preview/settings-macos-light.png).

## Workspace

- **Browse:** linked storage map and compact file list, breadcrumbs, name/size sorting, search of loaded items, and incremental loading. Hide the map or navigation sidebar to give the list more space.
- **Large files:** size-sorted files throughout the scanned locations, including files inside summarized ignored directories. Choose a minimum size, search loaded results, copy paths, or open the containing folder.
- **Developer cleanup:** Node dependencies, Cargo build output, Python bytecode, pytest/mypy/Ruff caches, and common JavaScript build caches. Rust and JavaScript candidates require project manifests; Python environments and uv caches are excluded from automatic cache classification.
- **Gitignored space:** the total and list of paths matched by the engine's `.gitignore` traversal. This does not include every Git global exclusion or prove that a file is disposable. Cache and ignored totals can overlap and are not added together.

Nothing is automatically selected. Selection is shared across views; selecting a parent replaces its selected descendants. **Select shown** applies only to the currently loaded and filtered items. Review shows the host's exact cleanup plan before **Move to Trash**. Results list moved and failed paths. Rescan afterward to update the storage map. Moving files to Trash does not itself guarantee immediately available disk space.

## Settings

Automatic uses Web styling in browsers and the platform's macOS/Windows treatment in Tauri. Override it with Web, macOS, or Windows on any platform. These are visual treatments of the shared Svelte components, not native OS widget implementations. Light/dark/system appearance and compact/comfortable density are saved locally.

## Development

```sh
# Terminal 1: read-only host, paired with the development UI
node --input-type=module -e "import { runServe } from './apps/tui/src/serve-entry.ts'; await runServe(['--host','loopback','--port','9421','--ui-origin','http://127.0.0.1:5173','--root',process.cwd()]);"

# Terminal 2: same-origin Vite API proxy
cd apps/web
SPACLENS_DEV_API=http://127.0.0.1:9421 yarn dev --host 127.0.0.1 --port 5173
```

Open the host's pairing URL with its `api` parameter set to `http://127.0.0.1:5173` when using this proxy. Add `--allow-cleanup` to the host only when you intend to enable cleanup.

The workbench requests protected local scans. The HTTP host runs the release Rust scanner and derives discovery from that measured report, without revisiting the filesystem. Tauri uses the protected Rust library; discovery is currently unavailable for native protected scans. Full startup-disk scans disable `.gitignore` content reads, so gitignored discovery is unavailable there; scan a local project folder to classify its ignore rules. Hidden local items are included and symlinks are not followed.

## Validation

From the repository root:

```sh
node --import tsx --test apps/web/test/selection.test.ts apps/web/test/appearance.test.ts packages/web-ui/test/sunburst.test.ts
node node_modules/svelte-check/bin/svelte-check --workspace apps/web --tsconfig ./tsconfig.json
```

`test/browser-fixture.ts` creates disposable files in a temporary directory and serves the built browser app on port 9423. Its simulated Trash moves files to a separate recovery folder, and `.ruff_cache` deliberately reports a failure for legacy cleanup-flow testing. Protected local scans are read-only. The fixture never uses the OS Trash or scans the user's repository:

```sh
yarn workspace @space-lens/web build
node --import tsx apps/web/test/browser-fixture.ts
```

Cloud and full-disk scan boundaries are documented in [CLOUD_SCAN_RESEARCH.md](./CLOUD_SCAN_RESEARCH.md). The new local-only mode disables macOS dataless materialization on every scanning thread, rejects known cloud roots before metadata, and excludes child mounts and symlinks. Coverage reports permission failures and unknown sizes separately. It does not inspect cloud storage or enable cleanup. The legacy generic scanner and GPUI entry point remain outside this protected flow; release optimization alone is not a safety boundary.

The actual Macintosh HD release stress run and memory measurements are recorded in [FULL_DISK_SCAN_REPORT.md](./FULL_DISK_SCAN_REPORT.md). The large report uses per-node NDJSON rather than a single giant JSON message. Disk-size displays use IEC units, and duplicate directory aliases are counted once.
