#!/bin/sh
set -eu

if [ "$(uname -s)" != Darwin ]; then
  printf '%s\n' 'This preview helper requires macOS.' >&2
  exit 1
fi

gpui_dir=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cargo build --manifest-path "$gpui_dir/Cargo.toml" --target-dir "$gpui_dir/target"
preview_dir=$(mktemp -d "${TMPDIR:-/tmp}/spacelens-gpui.XXXXXX")
bundle="$preview_dir/Space Lens.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$gpui_dir/target/debug/spacelens-gpui" "$bundle/Contents/MacOS/spacelens-gpui"
cp "$gpui_dir/../desktop/src-tauri/icons/icon.icns" "$bundle/Contents/Resources/SpaceLens.icns"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>dev.spacelens.gpui.preview</string>
  <key>CFBundleName</key><string>Space Lens</string>
  <key>CFBundleDisplayName</key><string>Space Lens</string>
  <key>CFBundleExecutable</key><string>spacelens-gpui</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleIconFile</key><string>SpaceLens</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>NSPrincipalClass</key><string>NSApplication</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
printf '%s\n' "$bundle"
