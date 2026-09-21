# Building the Space Lens desktop app

One SvelteKit SPA, two flavors (see `apps/web/svelte.config.js`); the desktop
flavor is embedded into the Tauri shell at compile time.

## Install (after the first `app-v*` release exists)

```bash
# Homebrew (cask source of truth: packaging/homebrew/Casks/space-lens.rb,
# pushed to HuakunShen/homebrew-tap per release with real version + sha256)
brew install --cask HuakunShen/homebrew-tap/space-lens

# or grab the dmg/NSIS/deb/AppImage from the GitHub release the tag built
```

The bundled updater checks `releases/latest/download/latest.json` (minisign
public key committed in `tauri.conf.json`; the private key lives only in the
`TAURI_SIGNING_PRIVATE_KEY*` GitHub secrets).

```bash
# 1. web: build the desktop flavor (build-desktop/, index.html fallback)
yarn workspace @space-lens/web build:desktop

# 2. shell: compile + bundle (own Cargo workspace under apps/desktop/src-tauri)
npx -y @tauri-apps/cli@2 build
```

Or run both with the orchestrator:

```bash
node scripts/build-desktop.ts
```

## Known issue (open)

`sl_read` responses from the third invoke onward never resolve on the JS
side, although the Rust command completes (`completed: true` probe at every
return path). Reproduced with `__TAURI_INTERNALS__.invoke`, with
`@tauri-apps/api/core`, with a serialized invoke queue, with sync and async
commands, on latest tauri/wry. macOS delivers `invoke` over a custom-protocol
fetch (`http://ipc.localhost`); the response for later fetches is lost between
the WKURLSchemeHandler and the page — suspected upstream WKWebView race
(tauri-apps/wry#1537).

Planned fix: switch `sl_read`/`sl_submit` responses to a Tauri **`Channel`**
(passed as a command argument — delivery rides the event system, not the fetch
response), or pin a wry version without the race. Traces: invoke #1/#2
resolve, #3 pends; probes at entry+exit both fire.

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
