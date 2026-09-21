fn main() {
    // This crate embeds apps/web/build-desktop at compile time; build the web
    // flavor first (scripts/build-desktop.ts enforces the order).
    tauri_build::build()
}
