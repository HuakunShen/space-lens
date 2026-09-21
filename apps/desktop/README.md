# Building the Space Lens desktop app

One SvelteKit SPA, two flavors (see `apps/web/svelte.config.js`); the desktop
flavor is embedded into the Tauri shell at compile time.

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
