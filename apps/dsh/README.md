# Space Lens for the DeepSeek Harness

The Space Lens workbench inside the Harness Web UI: whatever directory the current
session is working in, scanned and explored by Space Lens's own SPA — the same app
`spacelens serve` puts in a browser, shown in a panel.

It is not a reimplementation. A real Space Lens server is started inside the Harness
host process out of `@space-lens/host` — the same wiring the `spacelens serve` CLI
uses — and its HTTP application answers behind `/space-lens` on the Harness web
server. The panel is Space Lens's own SPA in a frame.

```
Harness Web UI
  ├── sidebar "Disk" ──────────────► main panel  ┐
  └── session-header button ───────► right column ┘
                                                  │  frame: /space-lens/
                                                  ▼
                                    Harness web server (127.0.0.1:3080)
                                      └── /space-lens/*  →  Space Lens server (loopback, port 0)
                                                              ├── started from @space-lens/host
                                                              └── authenticated by its own ticket
```

## The folder of the open project

The host half watches every Session's `cwd`. When a panel asks for a frame, the
host resolves the directory — the named Session's, an explicit `dir`, or the most
recently active Session's — and does two things:

- **Starts one Space Lens server for it.** Scan roots are fixed at startup and the
  `ScanManager` refuses anything outside them; that containment is the product's
  security model, so the plugin starts *a server per directory* rather than
  reaching into it. "Which folder is open" becomes "which server answers this
  frame" — one loopback, port-0 listener per project, never restarted, torn down
  with the plugin.
- **Redirects the frame through a pairing ticket.** The SPA pairs automatically
  (`?pair=`), names this mount as its API base (`?api=`), and starts the scan of
  the root on its own (`?autoscan=1`). Opening the panel *is* scanning the folder.

Because a frame's API calls carry no query of their own, the directory travels
inside the API base — `/space-lens/d/<encoded dir>` — a segment that belongs to
this plugin alone; the upstream never sees it. Two Sessions' panels therefore
cannot be confused about which server they are talking to.

The API override is the root-relative `/space-lens/d/<dir>`, not an absolute HTTP
address. In the browser this stays on the Harness HTTP origin; in desktop it stays
on `dsh-app://app` and uses the shell's authenticated forwarding transport. That
preserves `connect-src 'self'` without CORS allowances or custom schemes in the
product's origin policy. The desktop forwarder validates the renderer origin and
removes it before HTTP forwarding. After this route's own Origin/Host/Fetch-Site
checks pass, the plugin restores a missing Origin from the checked HTTP authority
for ticket binding. Explicit origins are preserved; cross-site, opaque and foreign
origins are still refused, and API reads still need a bearer.

The server is started **read-only**: no cleanup scopes, so the embedded workbench
can look but never trash. Deleting from a panel a session opened implicitly should
never be the easy path.

## Prerequisites

- Node.js 22+ (CI uses Node 24) and Yarn 4.14.1 — the repo pins
  `packageManager: yarn@4.14.1`, so `corepack enable` then `yarn` gives you
  the right version.
- No Rust toolchain needed for the plugin build: `yarn install` pulls the
  prebuilt `space-lens` engine binaries via its optional platform packages
  (e.g. `space-lens-darwin-arm64`). Only rebuilding the engine itself
  (`yarn workspace space-lens build`) needs Rust.

## Building

From the repository root, install dependencies once, then build:

```
yarn install
yarn build:dsh
```

This runs `scripts/build-dsh-plugin.mjs` and produces:

- `apps/dsh/dist/host.js` — the host bundle (ESM, loaded once per Harness
  process via the package's `.` export),
- `apps/dsh/dist/client.js` — the client bundle (IIFE registering into the
  Web shell's module table via the `./client` export),
- `apps/dsh/dist/web/` — the embedded SPA built for the `/space-lens` mount
  prefix (`SPACLENS_EMBED_BASE`), without a service worker,
- `apps/dsh/node_modules/space-lens` — the staged engine for local/link
  installs, resolved by Node walking up from `dist/host.js`.

`dist/` and the staged `node_modules/space-lens` are gitignored build
artifacts — they are **not** committed and **not** published. A checkout
without them (e.g. a fresh `git clone`) shows the plugin in the plugin list
with its toggle on, but no panel, because there is nothing for the host or
Web shell to load yet.

## Installing

From npm (the normal way — no build needed; the published package depends
on the `space-lens` engine from npm, so every platform installs its own
binary):

```
dsh plugin --profile desktop add dsh-plugin-spacelens
```

From this repository (development — build first, then link the directory;
`add` symlinks it, so the running Harness reads `dist/` from your checkout):

```
yarn build:dsh            # from the repository root
dsh plugin --profile desktop add /absolute/path/to/apps/dsh
```

(`dsh` here is the Harness CLI, e.g.
`/Applications/DeepSeek\ Harness.app/Contents/Resources/runtime/cli/bin/dsh`.)

Then restart the Harness app and check the plugin page. Two caching rules
make the restart mandatory, not optional:

- The host half is loaded once per Harness process and cached by package
  name — only a restart picks up a new `dist/host.js`.
- A bundle that failed to load once (e.g. installed before `dist/` existed)
  is skipped for the life of the process — a later rebuild alone will not
  resurrect it. Restart, then reload the page for the client half.

So: yes, publishing the npm package is enough for normal use. Local
`add <path>` installs are only for developing this plugin, and they require
`yarn build:dsh` first.

## Releasing

One tag publishes, same shape as the desktop line:

```
git tag plugin-v0.1.0 && git push origin plugin-v0.1.0
```

`.github/workflows/dsh-plugin.yml` builds the bundle and runs
`npm publish --access public` in `apps/dsh` with the `NPM_TOKEN`
secret; the tag's version must match `package.json`'s. Never commit the
staged engine: it lives in the package's root `node_modules`, which npm
excludes from tarballs unconditionally — the published package depends on
the `space-lens` npm package instead, so each platform installs its own
binary.

## Reloading it while developing

The host half is loaded once per Harness process and cached by package name —
only a restart picks up a new `dist/host.js`. The client half is served to the
browser and a page reload picks up a rebuild — unless the bundle failed to load
once, in which case it is skipped for the life of the process. `build-dsh-plugin.mjs`
writes a bundle only when its bytes changed, so rebuilding the SPA never poisons
the running Harness. See the Refyard plugin's README for the full story; the
constraints here are the same.

## Layout

```
package.json       bundle manifest: the patch, the client half, display metadata
cordis.patch.yml   the one host entry this bundle inserts
icon.svg           the mark the plugin list draws
locale/{en,zh}.json  display title and description
src/host.ts        host half: route, per-directory servers, pairing redirect, proxy
src/client.ts      client half: sidebar entry, main panel, right-column tab, header button
src/harness.d.ts   ambient types for the Harness plugin surface
dist/              generated: host.js, client.js, web/ (the embedded SPA)
node_modules/      generated: the staged engine for local installs (never packed)
```
