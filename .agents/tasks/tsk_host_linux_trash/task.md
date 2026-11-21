---
spec_version: 0.1.0
kind: task
id: tsk_host_linux_trash
project_id: prj_general
title: "Host 0.4.0: Linux trash fallback without gio"
stage: done
parent_id: null
depends_on: []
progress:
  mode: manual
  percent: 100
links: []
created_at: 2026-11-20T21:26:28Z
updated_at: 2026-11-20T21:39:06Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-host-trash-004
    request_hash: sha256:bb91ce40919a05f65b6c1cf6323e63323aadc8eb11d2d7d6d0eeae91c5c9c4a9
    applied_at: 2026-11-20T21:26:28Z
  - op_id: trash-done-007
    request_hash: sha256:a3f82ad3b7ab2d1a15a4c8d87eed2a5fde468e91c6bc0683afa7843eeecd3db7
    applied_at: 2026-11-20T21:39:06Z
checkpoint:
  summary: freedesktop fallback (files/ + info/*.trashinfo, EXDEV copy+remove, gio kept first choice) landed in 47e1657; 5 vitest cases green; verified live on ufo (no gio) via remote cleanup --execute.
  reported_at: 2026-11-21T05:46:00Z
  reporter: agent
---
## Scope
packages/host/src/trash.ts: when `gio` is unavailable, move entries into ~/.local/share/Trash per the freedesktop spec (files/ + info/*.trashinfo). Keep gio as the first choice.

## Acceptance criteria
- Unit tests for the fallback path; cleanup execute works on a headless Linux host (verified on ufo).
