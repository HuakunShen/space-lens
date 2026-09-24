# Space Lens macOS app

This is the native SwiftUI macOS app for Space Lens. It keeps the Phase 0
synthetic map as a safe demo, and now also supports a read-only real-folder
scan through the Rust FFI bridge.

The app can scan a user-selected folder without modifying it, render the
result in the Canvas Sunburst and inspector list, and export the portable JSON
snapshot. Selected real nodes can be staged in the Collector and moved to the
macOS Trash after an explicit review. Permanent deletion and MCP execution are
not part of this app slice yet. The iCloud Local Copies capability is backed by
the macOS-only Rust cloud adapter through the FFI session; its execution uses
the bounded Rust worker pool (8 by default, capped at 32).

```bash
just space-lens-mac-test
just space-lens-mac-build
just space-lens-mac-open
```

The Sunburst renderer uses one Canvas and a cached segment layout, with
per-segment hover hit shapes that show the item name, allocated size, root
percentage, and depth. The inspector list is the accessibility and keyboard-
equivalent representation of the chart. The release build links the Rust FFI library from
`packages/space-lens-ffi` and produces `.build/Space Lens.app`.
