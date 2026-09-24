#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CONFIGURATION="${CONFIGURATION:-release}"
PRODUCT="SpaceLensApp"
BUILD_DIR="$ROOT_DIR/.build"
APP_DIR="$BUILD_DIR/Space Lens.app"
ICONSET_DIR="$BUILD_DIR/SpaceLens.iconset"

cd "$ROOT_DIR"
if [[ "$CONFIGURATION" == "release" ]]; then
    cargo build -p space-lens-ffi --release
else
    cargo build -p space-lens-ffi
fi
export SPACE_LENS_FFI_LIB_DIR="$ROOT_DIR/../../target/$CONFIGURATION"
swift build -c "$CONFIGURATION" --product "$PRODUCT"

EXECUTABLE="$BUILD_DIR/$CONFIGURATION/$PRODUCT"
RESOURCE_BUNDLE="$BUILD_DIR/$CONFIGURATION/space-lens-mac_SpaceLensMacApp.bundle"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"
cp "$EXECUTABLE" "$APP_DIR/Contents/MacOS/$PRODUCT"
cp "$ROOT_DIR/Resources/Info.plist" "$APP_DIR/Contents/Info.plist"
cp "$ROOT_DIR/Resources/SpaceLensLogo.png" "$APP_DIR/Contents/Resources/SpaceLensLogo.png"

rm -rf "$ICONSET_DIR"
mkdir -p "$ICONSET_DIR"
for ICON_SIZE in 16 32 128 256 512; do
    DOUBLE_SIZE=$((ICON_SIZE * 2))
    sips -z "$ICON_SIZE" "$ICON_SIZE" "$ROOT_DIR/Resources/SpaceLensLogo.png" \
        --out "$ICONSET_DIR/icon_${ICON_SIZE}x${ICON_SIZE}.png" >/dev/null
    sips -z "$DOUBLE_SIZE" "$DOUBLE_SIZE" "$ROOT_DIR/Resources/SpaceLensLogo.png" \
        --out "$ICONSET_DIR/icon_${ICON_SIZE}x${ICON_SIZE}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET_DIR" -o "$APP_DIR/Contents/Resources/SpaceLens.icns"
rm -rf "$ICONSET_DIR"

if [[ -d "$RESOURCE_BUNDLE" ]]; then
    rm -rf "$APP_DIR/Contents/Resources/space-lens-mac_SpaceLensMacApp.bundle"
    cp -R "$RESOURCE_BUNDLE" "$APP_DIR/Contents/Resources/"
fi

codesign --force --deep --sign - "$APP_DIR" >/dev/null
printf '%s\n' "$APP_DIR"
