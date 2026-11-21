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
updated_at: 2026-11-21T02:28:56Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-gui-acceptance-001
    request_hash: sha256:af41af02f09e281c50439ccc53f151eae1d23bd751a45edc024cf474dde61eba
    applied_at: 2026-11-21T02:10:18Z
  - op_id: gui-acceptance-cp-001
    request_hash: sha256:b8f2da12d2c26a9e37706b3a9ae3f2973e0bc879253a268d80b6f63df806dc87
    applied_at: 2026-11-21T02:10:44Z
  - op_id: gui-cleanup-001
    request_hash: sha256:4e73fba2ca8cd1093f36c19cbab2c402d949009b0c0de4163866133f3f8bb404
    applied_at: 2026-11-21T02:28:56Z
checkpoint:
  summary: "GUI remote cleanup enabled and verified on ufo: plan 8.01 MiB → Move to Trash → node_modules.3 + .trashinfo in the remote trash, source gone. Desktop lib 78, host 217, web 79 green."
  reported_at: 2026-11-21T02:40:00Z
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

## Remote cleanup, enabled on the owner's call (2026-11-21)

The owner asked for it, so the desktop now maps trash-only cleanup:

- `connect` uses `RemotePolicy { allow_cleanup: true }`, so the remote host is served with `--allow-cleanup`;
- `submit_route` maps `cleanupPlan` → `/api/v1/cleanup/plan` and `cleanupExecute` → `/api/v1/cleanup/execute` (app cleanup, app scans and process termination stay unmapped);
- `remote_capabilities` passes the host's `cleanup` block through instead of masking it, so a host served read-only still shows no cleanup;
- the workbench starts a **non-protected** scan for a remote device (`localOnly: false`): the host refuses to plan against a protected scan by contract, and this matches what `spacelens remote cleanup` does. Local desktop/browser scans stay protected.

Live evidence (same Tauri build, device ufo):

```
scan /home/hk/space-lens-test  40.0 MiB (media.bin 32 + node_modules 8.01), footer "Cleanup enabled"
select node_modules            checkbox → Review → 1 item · 8.01 MiB
Review cleanup plan            "Confirm move to Trash — 1 item · 8.01 MiB in the verified plan"
Move to Trash                  "1 item moved to Trash · 8.01 MiB"
ufo: ls ~/space-lens-test      media.bin, package.json only (node_modules gone)
ufo: ~/.local/share/Trash      files/node_modules.3 + node_modules.3.trashinfo
```

Copy fixed on the way: the footer said "Local cleanup enabled" for any non-protected scan
(now device-aware) and the outcome card said "1 items".

## Verification
- Desktop lib tests: see checkpoint below.
- Client/desktop typecheck via the dev build (the app compiled and ran this code).
