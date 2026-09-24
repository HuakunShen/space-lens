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

The server is started **read-only**: no cleanup scopes, so the embedded workbench
can look but never trash. Deleting from a panel a session opened implicitly should
never be the easy path.

## Building

```
yarn build:dsh            # from the repository root
```

That produces, in order: the embedded SPA (`apps/web/build-embed`, built for the
`/space-lens` base without a service worker), the host bundle (`dist/host.js`,
with the napi engine staged at `dist/node_modules/space-lens` so the scan worker
can resolve it from inside the bundle), and the client bundle (`dist/client.js`).

Install it into the current Harness profile:

```
plugin_manager install_bundle  /absolute/path/to/integrations/dsh
```

Then check the bundle list and enable it explicitly if it is missing:

```
plugin_manager set_bundle  enabled=true  target=dsh-plugin-spacelens
```

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
dist/              generated: host.js, client.js, web/ (the embedded SPA), node_modules/ (the engine)
```
