---
spec_version: 0.1.0
kind: task
id: tsk_gui_remote_acceptance
project_id: prj_general
title: "Desktop GUI remote acceptance: drive ufo through the Tauri app"
stage: done
parent_id: null
depends_on: []
progress:
  mode: manual
  percent: 100
links: []
created_at: 2026-11-21T02:10:18Z
updated_at: 2026-11-21T02:10:44Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-gui-acceptance-001
    request_hash: sha256:af41af02f09e281c50439ccc53f151eae1d23bd751a45edc024cf474dde61eba
    applied_at: 2026-11-21T02:10:18Z
  - op_id: gui-acceptance-cp-001
    request_hash: sha256:b8f2da12d2c26a9e37706b3a9ae3f2973e0bc879253a268d80b6f63df806dc87
    applied_at: 2026-11-21T02:10:44Z
checkpoint:
  summary: "GUI remote verified on ufo: connect → browse /home/hk → space-lens-test → 3 items · 32.0 MiB scanned (media.bin 32 MiB, package.json 4 KiB). Desktop lib tests 78 passed. Local keeps the native dialog. Remote cleanup stays masked pending an owner decision."
  reported_at: 2026-11-21T02:11:00Z
  reporter: agent
---
## Scope
The desktop wiring existed (ceadab7) but no session had ever launched the Tauri app against a real remote host. This is the empirical pass: dev build, device dialog, browse, scan, results; plus the path picker reaching the GUI.

## What changed to make it true
- `ReadRequest::FsList { path }` (Tauri read arm) → `POST /api/v1/fs/list` in `apps/desktop/src-tauri/src/remote.rs`, so a remote desktop session can list folders.
- `listFolder` in the Tauri client (`packages/client/src/tauri.ts`).
- Local sessions still refuse it (their capabilities never advertise `host.pathList`), so the desktop keeps the native dialog there rather than offering a browser that cannot list.

## Evidence (browse-free: driven through Cua Driver's accessibility snapshots)

```
app            apps/desktop/src-tauri (tauri dev), window "Space Lens", pid 16708
env            SPACELENS_REMOTE_AGENT=<musl build>, SPACELENS_REMOTE_HOST_TARBALL=<0.4.0 tarball>
device dialog  SSH hosts: orb | ufo | mac-mini | rog16-windows | xross-*   (from ~/.ssh/config)
click Connect  switcher → "ufo", scan root /home/hk, Locations: hk only
Browse         AXList "Folders in /home/hk" — .1password, .agents, .android, .aws, .cargo …
filter         "space-lens" → .space-lens | space-lens-test
open           breadcrumb hk › space-lens-test; media.bin 32.0 MiB | package.json 44 B
Use this folder → Folder = /home/hk/space-lens-test; Scan enabled
Scan           "3 items · 32.0 MiB scanned", sunburst 32.0 MiB, contents media.bin 32.0 MiB + package.json 4.00 KiB
footer         "32.0 MiB scanned · Read-only scan"
switch back    This machine → Folder row shows "Choose…" (native dialog), no Browse
```

## Not available in the GUI, by construction
`apps/desktop/src-tauri/src/remote.rs` connects with `RemotePolicy::read_only()`, maps no mutation besides starting a scan, and masks `cleanup.*` to `none`; the dialog says "Remote hosts are read-only in this beta". So remote cleanup from the GUI is a product decision, not a bug: it needs `allow_cleanup` on the serve side plus plan/execute mapping, and the trash-only guarantee comes from the remote host's own cleanup. Owner call.

## Verification
- Desktop lib tests: see checkpoint below.
- Client/desktop typecheck via the dev build (the app compiled and ran this code).
