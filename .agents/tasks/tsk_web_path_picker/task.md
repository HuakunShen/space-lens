---
spec_version: 0.1.0
kind: task
id: tsk_web_path_picker
project_id: prj_general
title: "Web path picker: browse folders instead of typing absolute paths"
stage: done
parent_id: tsk_web_uiux_polish
depends_on: []
progress:
  mode: manual
  percent: 100
links: []
created_at: 2026-11-21T01:39:07Z
updated_at: 2026-11-21T01:58:57Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-path-picker-001
    request_hash: sha256:d25ad3e10d0c6dda5e94c1041594e009800800ea1202ef796bf99e6538563693
    applied_at: 2026-11-21T01:39:07Z
  - op_id: devlog-tsk_web_path_picker-1-1795226218
    request_hash: sha256:1df650e944ffbd48eecb4cc15e73e6fd9953c93039b8bfc5d441c7d9431c403d
    applied_at: 2026-11-21T01:56:58Z
  - op_id: update-picker-evidence-001
    request_hash: sha256:fbe4702d221b1cc645398aebe9fa17c88a0671904ff5a8d4bd4e8d2fab03c9ad
    applied_at: 2026-11-21T01:57:24Z
  - op_id: close-picker-001
    request_hash: sha256:741274b1d6da0cd1012b189320dffac939b8b19f1540e24b675fb200d572ce05
    applied_at: 2026-11-21T01:58:57Z
checkpoint:
  summary: "Picker shipped and verified locally and on ufo; polish folded in. Suite: host 217, web 79, svelte-check clean."
  reported_at: 2026-11-21T02:00:00Z
  reporter: agent
---
## Scope
The scan form today is one text input per folder plus a `Scan` button; the host will refuse anything outside its roots, and on a remote host the user has no idea what the paths look like. Owner reference: opencode's project picker (screenshot: search field, breadcrumb `/ > Users > hk`, folder rows with a chevron, Cancel / Select folder).

Build the same interaction against the host:

- **Host API**: `POST /api/v1/fs/list` `{ path }` → `{ path, parent, entries: [{ name, path, kind, size }] }`. Containment against the served roots (same rule as scans, including realpath so a symlink cannot escape), dirs first, deterministic sort, bounded page, no recursion. Gated like the other read-only routes (session token only).
- **Contract**: request/response schemas in `@space-lens/contract`, plus the client method in the web service layer.
- **UI** (`ScanPicker.svelte` + route wiring): breadcrumb with clickable crumb (root → … → current), list rows with folder icon and a chevron to descend, a filter field that matches the current folder's children, recent locations as chips, keyboard `↑/↓` move `Enter` descend/select `Esc` close, and a primary `Scan this folder` action. Multi-folder (second folder) stays available but secondary.
- Works for local and remote hosts with no code path difference (it is the host that lists).

## Acceptance criteria
- From a fresh workbench, a scan of a nested folder is possible with zero typing: roots → drill → Scan.
- Filtering narrows the visible rows and Enter picks the highlighted one.
- A path outside the roots cannot be reached or submitted; the host answers a structured refusal and the UI shows it inline rather than as a raw alert.
- Host tests cover: listing, containment refusal (including a symlink pointing outside), sorting, and hidden entries.
- Verified in the browser against the local serve and against ufo via `remote open`; snapshot evidence pasted below.

## Evidence

Host route and tests (local):

```
yarn workspace @space-lens/host test   → Test Files 27 passed | Tests 217 passed
apps/web/test/fs-list.test.ts          → 8 cases: dirs-first listing with sizes, parent at a
                                         root, outside-root refusal, symlink-escape refusal,
                                         missing folder vs file path, limit/truncated,
                                         tilde outside roots, nested roots
```

Browser pass, local serve (`serve --root ~ --port 9470`), driven with the browse extension:

```
Browse button            aria-expanded=false → true, aria-controls=scan-folder-browser
field name               Folder (was "Folder Browse" while the button sat inside the <label>)
listbox                  "Folders in /Users/hk" — 141 rows, .agent/.agents/.android… first
filter "dev"             .kunkun-dev | Dev | develop | ExtDev
click Dev                breadcrumb hk › Dev, rows CrossCopy | demo | kunkun | …
ArrowDown ×2 + Enter     opened /Users/hk/.android (aria-activedescendant fs-row-2)
Backspace                back to /Users/hk
typed /Users/hk/Dev ⏎   navigated there, filter cleared
Use this folder          input = /Users/hk/Dev, panel closed, Scan enabled
Escape                   panel closed, aria-expanded=false
```

Remote host (ufo, `remote open ufo`, host package reinstalled from the new tarball):

```
Browse → listbox "Folders in /home/hk"
filter "space-lens" → .space-lens | space-lens-test
click space-lens-test → breadcrumb hk › space-lens-test, rows media.bin 32.0 MiB | package.json 44 B
Use this folder → /home/hk/space-lens-test → Scan → "3 items · 32.0 MiB scanned", 32.0 MiB on disk
```

Polish found by that pass and fixed in the same commit: the label/button a11y name above,
folder rows showing a meaningless inode size (ext4 reports 4 KiB for every directory, so
sizes now render for files only), files rendered with a folder icon, and the chart
inspector's "a item".

Deferred (owner may overrule): `DEF-PICKER-1` recent-location chips inside the browser — the
sidebar already lists recent scans, so the browser starts at a served root instead; and
chevron-per-row expansion — drilling is a click, and each row's `Use` button picks a child
without entering it.

## Development log

<!-- dev-log v1: add rows with scripts/dev-log.py; the metrics table is recomputed, do not edit it by hand -->

### Process metrics

| Metric | Value |
|---|---|
| Started | 2026-11-21T01:56Z |
| Last event | 2026-11-21T01:56Z |
| Wall clock (min) | 0 |
| Recorded work minutes (sum of rows) | 95 |
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
| 1 | 2026-11-21T01:56Z | impl | deepseek-v4.1-flash | 95 | 1 | host fs/list + contract + HTTP client + PathBrowser + ScanPicker wiring; host 217 tests, web 79 tests, svelte-check clean; verified in the browser locally and against ufo over SSH | 5656881 |
