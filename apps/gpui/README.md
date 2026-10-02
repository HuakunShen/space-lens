# Space Lens · GPUI

The macOS desktop interface uses a single toolbar, a location sidebar, a sunburst explorer, and a segmented inspector. The app follows the system appearance; the toolbar can switch between light and dark.

From the repository root:

```sh
cargo run --manifest-path apps/gpui/Cargo.toml
```

This crate is its own workspace and uses the checked-out `vendors/kuntu` scan engine. On a fresh checkout, initialize the submodule before building:

```sh
git submodule update --init vendors/kuntu
```

For a macOS `.app` preview, run:

```sh
./apps/gpui/scripts/macos-preview.sh
```

The helper builds the app and copies it into a temporary bundle on the startup disk, then prints its location. Open that bundle in Finder. It does not install anything or alter the Tauri app. Copying the executable avoids slow Launch Services startup when the repository is on an external volume.

## Using the workbench

- **New Scan** opens the compact scan form. Choose a folder or enter its path; a second root is optional. Cancel immediately restores the form. Returning to an existing scan cancels the pending scan result.
- Select a folder in the chart or **Contents** to explore it. The breadcrumb returns to a parent. Summarized ignored folders remain identifiable.
- **Contents / Cleanup / Git** switch the inspector. Toolbar buttons show or hide the sidebar, inspector, and collector.
- Use **+** to collect paths. The collector opens automatically. **Plan Cleanup…** shows the total and a scrollable list of every planned path before the explicit **Move to Trash** action. Plans expire after ten minutes and fingerprints are verified again before execution. Completion reveals the result, including failures.

Recent folders are stored in `~/Library/Application Support/SpaceLens/recent-scans.json`. The GPUI shell uses its own fixed native layout; legacy dock layout files are not read.

## Checks and screenshot hooks

```sh
cargo test --offline --manifest-path apps/gpui/Cargo.toml
cargo build --offline --manifest-path apps/gpui/Cargo.toml
cargo fmt --manifest-path apps/gpui/Cargo.toml -- --check --config tab_spaces=2
```

For a controlled local screenshot session:

```sh
SPACLENS_GPUI_THEME=light SPACLENS_GPUI_AUTO_SCAN=/path/to/folder \
  cargo run --manifest-path apps/gpui/Cargo.toml
```

`SPACLENS_GPUI_THEME` accepts `light` or `dark`; without it the initial theme follows macOS. `SPACLENS_GPUI_AUTO_SCAN` pre-fills and scans one folder. These hooks do not supply sample scan data.

## Verified screenshots

The screenshots are captures of the running GPUI application:

- [Actual project, light](preview/gpui-native-project-light.png) · [dark](preview/gpui-native-project-dark.png)
- [Scan form, light](preview/gpui-native-picker-light.png) · [dark](preview/gpui-native-picker-dark.png)
- [Twelve-path confirmation](preview/gpui-native-confirm-light.png) · [scrolled to the last path](preview/gpui-native-confirm-scrolled-light.png)

The form and confirmation captures use a controlled temporary fixture. Live checks covered folder scanning, directory navigation, the native folder dialog, appearance changes, panel toggles, candidate collection, confirmation scrolling, cancellation, and removal from the queue. No trash execution was performed during these UI checks. The purple badge over the traffic-light area in automated captures is a macOS screen-control indicator.
