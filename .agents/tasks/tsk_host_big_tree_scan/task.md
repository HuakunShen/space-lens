---
spec_version: 0.1.0
kind: task
id: tsk_host_big_tree_scan
project_id: prj_general
title: Host napi scan exhausts memory on huge homes
stage: todo
parent_id: null
depends_on: []
progress:
  mode: manual
  percent: null
links: []
created_at: 2026-11-21T01:09:36Z
updated_at: 2026-11-21T01:09:36Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-bigtree-011
    request_hash: sha256:5109c1b1a2c28f7cd9c53e5aa5748be307afe7dd1a43ea67e4c23c63e02b186d
    applied_at: 2026-11-21T01:09:36Z
---
## Scope
Scanning ufo's 936.8 GiB home through the npm host killed the node serve process mid-scan (no survivor process, 64 GB RAM machine). The napi path builds the whole tree in memory; the desktop avoids this with its compact NodeIndex and eviction budget.

## Acceptance criteria
- Root cause confirmed (node heap limit vs tree size) with a live measurement on ufo.
- Either the host streams/budgets the tree, or interactive UX steers big-root scans to `localOnly`/scoped paths, documented.
- The CLI already warns: roots carry size, browse of a >50 GiB root asks twice (611c419).
