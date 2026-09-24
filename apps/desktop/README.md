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
