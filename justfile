set shell := ["zsh", "-euo", "pipefail", "-c"]

icloud-build:
    ./apps/icloud-free/scripts/build-app.sh

icloud-open:
    open ./apps/icloud-free/.build/ICloudFree.app

icloud-build-open:
    ./apps/icloud-free/scripts/build-app.sh
    open ./apps/icloud-free/.build/ICloudFree.app

icloud-test:
    cd apps/icloud-free && swift test

icloud-cli *args:
    cd apps/icloud-free && swift run icloud-free {{args}}

rust-cli *args:
    cargo run --release -p space-lens-cli -- {{args}}

space-lens-mcp:
    cargo run --release -p space-lens-cli --features mcp -- mcp

space-lens-mac-test:
    cargo build -p space-lens-ffi
    cd apps/space-lens-mac && SPACE_LENS_FFI_LIB_DIR="$PWD/../../target/debug" swift test

space-lens-mac-build:
    ./apps/space-lens-mac/scripts/build-app.sh

space-lens-mac-open:
    open "./apps/space-lens-mac/.build/Space Lens.app"

space-lens-mac-build-open:
    ./apps/space-lens-mac/scripts/build-app.sh
    open "./apps/space-lens-mac/.build/Space Lens.app"
