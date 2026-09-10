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
