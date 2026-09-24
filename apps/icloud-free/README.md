# iCloud Free

`iCloud Free` inspects iCloud Drive folders and shows which local file copies can be removed without deleting the iCloud items.

The GUI is a SwiftUI macOS app. On macOS 26 it uses SwiftUI Liquid Glass (`glassEffect`/`GlassEffectContainer`); older supported macOS versions use a translucent material fallback.

The CLI uses [Swift Argument Parser](https://github.com/apple/swift-argument-parser), so `--help`, subcommands, flags, validation, and usage text are generated rather than hand-written.

## Run the CLI

```bash
swift run icloud-free --help
swift run icloud-free status icloud:Pictures/lightroom --recursive
swift run icloud-free status icloud:Pictures/lightroom --recursive --json
swift run icloud-free evict icloud:Pictures/lightroom --recursive --dry-run
swift run icloud-free evict icloud:Pictures/lightroom --recursive --execute
```

Eviction is always a dry-run unless `--execute` is supplied. The execute path calls Foundation’s `FileManager.evictUbiquitousItem(at:)`; it does not use filesystem deletion APIs and never deletes iCloud items.

## Run the GUI

```bash
swift run ICloudFreeApp
```

Drop an iCloud Drive file or folder onto the window, or choose one with the folder button. The app scans recursively, shows local/cloud-only status, and requires a confirmation before freeing local copies.

While scanning or freeing local copies, the window shows the current item and progress. Use `Pause`/`Resume` to gate the worker between items, or `Stop` to cancel pending work. Click the `File`, `Status`, `Local`, or `Logical` table headers to sort the result list.

Eviction uses a bounded concurrent worker pool: eight items by default, capped at 32. It does not create one task per file. Each item keeps its own safety/error boundary; pause prevents workers from claiming more items, cancellation prevents new work, and progress reports active workers.

## Build a double-clickable app

```bash
./scripts/build-app.sh
open .build/ICloudFree.app
```

Set `CONFIGURATION=debug` or `APP_DIR=/some/output/ICloudFree.app` to customize the build.

From the repository root, the same workflow is available through `just`:

```bash
just icloud-build
just icloud-open
just icloud-build-open
CONFIGURATION=debug just icloud-build
just icloud-test
```

The bundle stays under `apps/icloud-free/.build/ICloudFree.app`; these recipes do not copy it to `/Applications`.
