---
spec_version: 0.1.0
kind: task
id: tsk_web_uiux_polish
project_id: prj_general
title: "Web UI/UX: browsable path picker, symlink follow, full-screen audit"
stage: doing
parent_id: null
depends_on: []
progress:
  mode: rollup
links: []
created_at: 2026-11-21T01:39:07Z
updated_at: 2026-11-21T01:39:07Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-uiux-parent-001
    request_hash: sha256:ba774363d1dd14aa7a13fde54e22cfa328c3b5afec6228095ede1203b0adb1f1
    applied_at: 2026-11-21T01:39:07Z
---
## Scope
Owner asked (2026-11-21) for the web workbench to stop requiring a hand-typed absolute path and to look like a real product: an opencode-style path selector, plus a browse-driven pass over every screen to fix what reads wrong.

Three lanes, each its own task:

- [tsk_web_path_picker](../tsk_web_path_picker/task.md) — host directory-listing endpoint + breadcrumb/type-to-filter picker (local and remote hosts alike).
- [tsk_scan_follow_symlinks](../tsk_scan_follow_symlinks/task.md) — `~/dev` is 13 symlinks; the contract, host napi worker and protected spawn all drop `followSymlinks`, so a scan returns 12 KiB.
- [tsk_web_uiux_audit](../tsk_web_uiux_audit/task.md) — drive the real UI with the browse extension (local serve + `remote open ufo`), log every rough edge, fix them.

## Acceptance criteria
- A scan can be started in the web UI without typing an absolute path, and the same picker works against a remote host.
- A folder of symlinks can be scanned with their real sizes on request.
- Every screen the audit visits is either fixed or recorded as `DEF-*` with a reason.
- `yarn workspace @space-lens/web svelte-check`, the web/host test suites and `cargo test -p spacelens` pass; browser evidence is pasted into the lane tasks.

## Focus
`docs/app-track/FOCUS.md` carries the live status of all three lanes.
