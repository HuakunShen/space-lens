---
spec_version: 0.1.0
kind: task
id: tsk_scan_follow_symlinks
project_id: prj_general
title: "Scan option: follow symlinks end to end (contract, host, UI, desktop)"
stage: todo
parent_id: tsk_web_uiux_polish
depends_on: []
progress:
  mode: manual
  percent: null
links: []
created_at: 2026-11-21T01:39:07Z
updated_at: 2026-11-21T01:39:07Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-follow-symlinks-001
    request_hash: sha256:aa3bceed40c10fe7ac067f656c56ef2adec5ffa574b04339f82a88b8d52a21b4
    applied_at: 2026-11-21T01:39:07Z
---
## Context
Owner scanned `~/dev` in the web UI and got 12 KiB: 13 of 16 entries are symlinks and the UI marks them `Not scanned · symlink`. Both engines already support it — `packages/node` `scanDirectory({ followSymlinks })` and `spacelens scan --follow-symlinks` (`ScanArgs.follow_symlinks`) — but the chain drops it:

- `packages/contract/src/scan.ts` `ScanStartRequestSchema` has no `followSymlinks`.
- `packages/host/src/scan-store.ts` hardcodes `followSymlinks: false` in the napi worker request.
- `packages/host/src/local-process.ts` `spawnLocalScan` never passes `--follow-symlinks`.
- `apps/web/src/routes/+page.svelte` / `ScanPicker.svelte` have no control.
- `apps/desktop/src-tauri/src/engine.rs` `ScanStartRequest` has no field.

Note: protected (`localOnly`) scans reject `follow_symlinks = true` at `vendors/kuntu/crates/kuntu-scan/src/local_scan.rs`.

## Acceptance criteria
- `ScanStartRequestSchema.followSymlinks` (default false) flows to the napi worker and to the protected spawn.
- A protected scan with `followSymlinks` reports a clear, non-crashing refusal (or the request is rejected up front with a message the UI can show), never a hang.
- UI: a clearly labelled toggle, off by default, with the duplicate-and-loop caveat; disabled with the reason when the scan mode cannot follow.
- Scanning `~/dev` with it on reports the real subtree sizes.
- Tests: contract default, host request plumbing, spawn args; suites green.
