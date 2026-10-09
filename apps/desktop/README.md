# Building the Space Lens desktop app

One SvelteKit SPA, two flavors (see `apps/web/svelte.config.js`); the desktop
flavor is embedded into the Tauri shell at compile time.

## Install (after the first `app-v*` release exists)

```bash
# Homebrew (cask source of truth: packaging/homebrew/Casks/space-lens.rb;
# each app-v* release attaches the rendered cask with real version + sha256
# to the GitHub release, and the owner copies it into HuakunShen/homebrew-tap)
brew install --cask HuakunShen/homebrew-tap/space-lens

# or grab the dmg/NSIS/deb/AppImage from the GitHub release the tag built
```

The bundled updater checks `releases/latest/download/latest.json` (minisign
public key committed in `tauri.conf.json`; the passwordless private key lives
only in the `TAURI_SIGNING_PRIVATE_KEY` GitHub secret — no password secret
exists, the empty default is correct).

```bash
# 1. web: build the desktop flavor (build-desktop/, index.html fallback)
yarn workspace @space-lens/web build:desktop

# 2. shell: compile + bundle (own Cargo workspace under apps/desktop/src-tauri)
yarn workspace @space-lens/desktop build
```

Or run both with the orchestrator:

```bash
node scripts/build-desktop.ts
```

For an unbundled local launch after building the desktop frontend:

```bash
cargo run --manifest-path apps/desktop/src-tauri/Cargo.toml --bin space-lens-desktop
```

The macOS host uses public `NSGlassEffectView.contentView` on macOS 26 and
newer, falling back to `NSVisualEffectView` with `underWindowBackground` on
older releases. It wraps the existing Tauri content container so WebView,
resize, keyboard and drag handles stay intact. Native objects remain on the
main thread and their parent views retain them; notification observers are
removed when the window closes.

WebKit transparency still requires the private `_setDrawsBackground:` setter.
The host checks the selector before calling it and leaves the window opaque
if it is unavailable. The page receives the actual installed capability in
`backdrop`/`data-backdrop`, plus `macos`, `dark` and
`--color-system-accent`. Its document-start contract begins with `none` and
updates after native installation while the window is hidden. Appearance and
accent notifications refresh the contract; the web layer decides how much of
the material to reveal while keeping content legible.

Validate the bootstrap's document-start timing and live state transitions:

```bash
node --test apps/desktop/src-tauri/src/native_material.test.mjs
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib
```

## IPC transport (resolved)

macOS delivers `invoke` over a custom-protocol fetch (`http://ipc.localhost`),
and from the third call onward the fetch RESPONSE never reached the page —
the Rust command completed, the JS promise pended forever. Fix: every `sl_*`
command now replies through a Tauri **`Channel`** (event delivery) and returns
`Ok(())` in the fetch body, which the frontend ignores entirely
(`packages/client/src/tauri.ts`).

Rules that hold for the native shell:

- No JS runtime ships in the product: the Svelte bundle is compiled in, and
  the engine is `kuntu-scan` linked directly (aliased as `space-lens`).
- IPC is a closed command set (`sl_*`); requests are tagged unions with
  `deny_unknown_fields`, so an unknown method is a typed problem, never an
  IPC deserialization crash.
- Cleanup is trash-only (`trash` crate); execution re-verifies every
  fingerprint and fails the whole plan closed on any mismatch.
- Subscriptions are host-minted (`sub_*`) and frames are window-scoped —
  window B cannot drive window A's session.

## Capability notes

- Discovery and cleanup are both available on the desktop, derived only from
  the local scan report/index: discovery never revisits the filesystem, and
  cleanup plans from the report with a plan-time fingerprint baseline that
  execution re-verifies.
- The declared `scan.maxConcurrent` is 1 on purpose: a single-user desktop
  runs one engine worker per session and rejects a second scan while one is
  active (slot semantics locked by
  `apps/desktop/src-tauri/tests/local_scan.rs::protected_prepare_is_nonblocking_and_cancellation_is_terminal`).
  The HTTP host defaults are higher (`maxConcurrentScans` 4 / `maxTotalScans`
  8, `packages/host/src/config.ts`) and are declared from configuration by
  `packages/host/src/server.ts`; the desktop shell deliberately does not
  expose a worker pool.
