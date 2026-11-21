---
spec_version: 0.1.0
kind: task
id: tsk_web_uiux_audit
project_id: prj_general
title: "UI/UX audit: drive every web screen with the browse extension and fix the rough edges"
stage: doing
parent_id: tsk_web_uiux_polish
depends_on: []
progress:
  mode: manual
  percent: null
links: []
created_at: 2026-11-21T01:39:07Z
updated_at: 2026-11-21T01:58:37Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-uiux-audit-001
    request_hash: sha256:ca7d6cb6b2affcbd14cc32b23150fcf9908beddb48cde15ddd29fe423323891d
    applied_at: 2026-11-21T01:39:07Z
  - op_id: update-audit-findings-001
    request_hash: sha256:a3bf332db139289e8e94aa5d7d090f7c7f153ad538698ce2ba232721825bee47
    applied_at: 2026-11-21T01:58:36Z
  - op_id: devlog-tsk_web_uiux_audit-1-1795226317
    request_hash: sha256:d2964077b1fff75a5b4321927935523a758e2b3a8b1f84c4f3fe9dd5647d057e
    applied_at: 2026-11-21T01:58:37Z
checkpoint:
  summary: "impl 1 by deepseek-v4.1-flash: first pass: scan form + path browser on local serve and ufo; 5 findings fixed (5656881), 3 open"
  reported_at: 2026-11-21T01:58:37Z
  reporter: deepseek-v4.1-flash
---
## Scope
The owner handed browser control over, so the audit is empirical, not speculative: pair the `browse` extension, open the local serve and a `remote open ufo` session, and walk every surface — landing/locations, scan form, scanning progress, results (chart types, folder contents), selection/review, cleanup plan and execute, discovery (large files, caches, gitignored), apps audit, settings, error states, empty states, keyboard reachability, `prefers-reduced-motion` — plus narrow and wide viewports.

For each finding: id, screen, what is wrong, severity (P1 blocks a task / P2 rough / P3 polish), and either a fix commit or a `DEF-*` deferral with the reason. Anything that is a data-correctness bug (wrong size, wrong path, missing entries) jumps the queue.

## Acceptance criteria
- A findings table in this task with severity, status and commit/deferral per row.
- Every P1 and P2 either fixed and re-verified in the browser, or explicitly deferred by the owner.
- The scan form needs no typed absolute path (depends on `tsk_web_path_picker`).
- Both hosts (local serve, ufo) reach the same screens without a raw error string in the UI.
- Fixes keep the suites green (`svelte-check`, vitest, host, cargo) and are committed separately from documentation.

## Findings

Passes so far: local serve (`serve --root ~`, port 9470) and ufo (`remote open ufo`, host
package reinstalled), driven by the browse extension. Severity: P1 blocks a task, P2 rough,
P3 polish.

| id | screen | finding | severity | status |
|---|---|---|---|---|
| UX-1 | scan form | The only way to choose a folder was typing an absolute path — impossible to discover on a remote host. | P2 | fixed in 5656881 (PathBrowser: breadcrumb, filter, keyboard, `Use this folder`) |
| UX-2 | scan form | The `Browse` button lived inside the folder `<label>`, so the field's accessible name became "Folder Browse" and label clicks hit the button. | P3 | fixed in 5656881 (real `<label for>`, `aria-expanded`/`aria-controls`) |
| UX-3 | path browser | Folder rows printed their inode size — ext4 reports 4 KiB for every directory, so `/home/hk` looked like `.cargo 4.00 KiB`. | P3 | fixed in 5656881 (sizes render for files only) |
| UX-4 | path browser | Files were drawn with a folder icon. | P3 | fixed in 5656881 |
| UX-5 | results chart | Inspector empty state said "Hover or focus a item" (the noun is configurable). | P3 | fixed in 5656881 (article by vowel) |
| UX-6 | results | Scanning a folder of symlinks (`/Users/hk/Dev`) reports "0 items · 12.0 KiB" / "5 items · 12.0 KiB" with no on-screen reason; the empty state says "No child items in this view." | P2 | open — belongs with tsk_scan_follow_symlinks; the results view should name skipped symlinks |
| UX-7 | landing | Nothing warns before scanning a huge root from the browser; scanning ufo's `/home/hk` (936.8 GiB) killed the remote serve. The CLI warns (root size shown, >50 GiB browse asks twice). | P2 | open — needs a size the web page does not have yet; track with tsk_host_big_tree_scan |
| UX-8 | recent scans | Clicking a recent scan re-opens the scan form ("Scan root …" + `Scan`) instead of re-running it in one action, and the button does not say "Rescan". | P3 | open |

## Evidence log

```
host tests   Test Files 27 passed | Tests 217 passed
web tests    tests 79 | pass 79   (gates + fixture suites)
svelte-check 0 errors, 0 warnings

local: /Users/hk → filter "dev" → Dev → breadcrumb hk › Dev → ArrowDown×2 ⏎ opened .android
       Backspace back, typed /Users/hk/Dev ⏎ navigated, Use this folder → Scan enabled
ufo:  Browse → "Folders in /home/hk" → filter space-lens → space-lens-test →
       media.bin 32.0 MiB | package.json 44 B → Use → Scan → "3 items · 32.0 MiB scanned"
results (local, /Users/hk/Dev): "5 items · 12.0 KiB scanned", "No child items in this view."
```

## Development log

<!-- dev-log v1: add rows with scripts/dev-log.py; the metrics table is recomputed, do not edit it by hand -->

### Process metrics

| Metric | Value |
|---|---|
| Started | 2026-11-21T01:58Z |
| Last event | 2026-11-21T01:58Z |
| Wall clock (min) | 0 |
| Recorded work minutes (sum of rows) | 40 |
| Implementation runs | 1 |
| Review rounds | 0 (GO 0, NO-GO or changes 0) |
| Fix rounds | 0 |
| Verify runs | 0 |
| Merges | 0 |
| Blocked minutes | 0 |
| Deferred items | none |
| Who ran it | deepseek-v4.1-flash |

### Timeline

| # | Time (UTC) | Kind | Who | Minutes | Round | Outcome | Ref |
|---|---|---|---|---|---|---|---|
| 1 | 2026-11-21T01:58Z | impl | deepseek-v4.1-flash | 40 | 1 | first pass: scan form + path browser on local serve and ufo; 5 findings fixed (5656881), 3 open | 5656881 |
