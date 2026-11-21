---
spec_version: 0.1.0
kind: task
id: tsk_host_big_tree_scan
project_id: prj_general
title: Host napi scan exhausts memory on huge homes
stage: doing
parent_id: null
depends_on: []
progress:
  mode: manual
  percent: 70
links: []
created_at: 2026-11-21T01:09:36Z
updated_at: 2026-11-21T04:49:17Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-bigtree-011
    request_hash: sha256:5109c1b1a2c28f7cd9c53e5aa5748be307afe7dd1a43ea67e4c23c63e02b186d
    applied_at: 2026-11-21T01:09:36Z
  - op_id: update-bigtree-001
    request_hash: sha256:5cda66872ad3cc11ad86a25e5e3db02e0306bcaf834b19e8e2d8ed35afb16671
    applied_at: 2026-11-21T04:49:17Z
checkpoint:
  summary: "Root causes found: (1) contract rejected 4184-char paths with 'Too big' and aborted the scan; (2) the CLI's `remote scan` uses the napi whole-tree engine, which is what OOM'd. The browser's protected streaming path scans ufo ~/dev (5,048,182 items, 170 GiB) with the host at ~720 MB RSS."
  reported_at: 2026-11-21T04:50:00Z
  reporter: agent
---
## Findings (2026-11-21, ufo)
`~/dev` is **182 GB / 5.12M paths**. Two distinct failures, neither a single "memory" bug:

1. **Contract path ceiling (fixed, 0.4.3).** A nested `node_modules` tree holds paths of **4184 characters**; `LocalScanNodeSchema.path` capped at 4096, so the protected stream's first over-long node aborted the whole scan with `invalid protected scanner protocol: Too big: expected string to have <=4096 characters`. 4096 is PATH_MAX for one syscall, not a real maximum (`find` walks with openat). Ceiling raised to 16384 for scan/local-scan/cleanup/fs paths, with a contract test.
2. **CLI napi path (open).** `spacelens remote scan` starts an ordinary (non-protected) scan, which builds the whole tree in a worker — that is what OOM'd on the 936 GiB home. The browser sends `localOnly: true`, which streams NDJSON nodes and indexes incrementally; that path survives.

## Verified after the fix
The browser workbench scanned `/home/hk/dev` on ufo: **5,048,182 items · 170 GiB scanned** (187 GiB logical), 105 top-level entries, no alert; host process peaked ~721 MB RSS on a 64 GB machine. `others`, `CrossCopy` show `partial` (permission-denied subdirs) and their review checkboxes are disabled — correct.

## Remaining
- [ ] `remote scan --protected` (or auto-select the streaming path above a size/node threshold) so the CLI can scan huge trees too.
- [ ] Decide whether a protected scan may be planned for cleanup; today the host refuses it, which is why remote cleanup uses an ordinary scan.
- [ ] Budget/evict the index for trees well beyond 5M nodes (whole home on a 1 TB box).
