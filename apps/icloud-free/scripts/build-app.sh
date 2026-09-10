#!/bin/zsh
set -euo pipefail

SCRIPT_DIR="${0:A:h}"
PACKAGE_DIR="${SCRIPT_DIR:h}"
CONFIGURATION="${CONFIGURATION:-release}"
APP_DIR="${APP_DIR:-${PACKAGE_DIR}/.build/ICloudFree.app}"

cd "$PACKAGE_DIR"
swift build -c "$CONFIGURATION" --product ICloudFreeApp
BIN_DIR="$(swift build -c "$CONFIGURATION" --show-bin-path)"
ICON_SOURCE="$PACKAGE_DIR/Sources/ICloudFreeApp/Resources/SpaceLensLogo.png"
ICONSET_DIR="$PACKAGE_DIR/.build/SpaceLens.iconset"
RESOURCE_BUNDLE="$BIN_DIR/icloud-free_ICloudFreeApp.bundle"
RESOURCE_DEST="$APP_DIR/Contents/Resources/icloud-free_ICloudFreeApp.bundle"

mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"
cp "$BIN_DIR/ICloudFreeApp" "$APP_DIR/Contents/MacOS/ICloudFreeApp"
cp "$PACKAGE_DIR/Resources/Info.plist" "$APP_DIR/Contents/Info.plist"
cp "$ICON_SOURCE" "$APP_DIR/Contents/Resources/SpaceLensLogo.png"
/usr/bin/ditto "$RESOURCE_BUNDLE" "$RESOURCE_DEST"

mkdir -p "$ICONSET_DIR"
/usr/bin/sips -z 16 16 "$ICON_SOURCE" --out "$ICONSET_DIR/icon_16x16.png" >/dev/null
/usr/bin/sips -z 32 32 "$ICON_SOURCE" --out "$ICONSET_DIR/icon_16x16@2x.png" >/dev/null
/usr/bin/sips -z 32 32 "$ICON_SOURCE" --out "$ICONSET_DIR/icon_32x32.png" >/dev/null
/usr/bin/sips -z 64 64 "$ICON_SOURCE" --out "$ICONSET_DIR/icon_32x32@2x.png" >/dev/null
/usr/bin/sips -z 128 128 "$ICON_SOURCE" --out "$ICONSET_DIR/icon_128x128.png" >/dev/null
/usr/bin/sips -z 256 256 "$ICON_SOURCE" --out "$ICONSET_DIR/icon_128x128@2x.png" >/dev/null
/usr/bin/sips -z 256 256 "$ICON_SOURCE" --out "$ICONSET_DIR/icon_256x256.png" >/dev/null
/usr/bin/sips -z 512 512 "$ICON_SOURCE" --out "$ICONSET_DIR/icon_256x256@2x.png" >/dev/null
/usr/bin/sips -z 512 512 "$ICON_SOURCE" --out "$ICONSET_DIR/icon_512x512.png" >/dev/null
/usr/bin/sips -z 1024 1024 "$ICON_SOURCE" --out "$ICONSET_DIR/icon_512x512@2x.png" >/dev/null
/usr/bin/iconutil -c icns "$ICONSET_DIR" -o "$APP_DIR/Contents/Resources/SpaceLens.icns"
/usr/bin/codesign --force --deep --sign - "$APP_DIR" >/dev/null

echo "$APP_DIR"
