---
spec_version: 0.1.0
kind: task
id: tsk_mcp_remote_cleanup
project_id: prj_general
title: "MCP: remote device tools and opt-in cleanup"
stage: done
parent_id: null
depends_on: []
progress:
  mode: manual
  percent: 100
links: []
created_at: 2026-11-20T21:26:28Z
updated_at: 2026-11-20T22:05:25Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-mcp-remote-001
    request_hash: sha256:bf76ce2d983e60a91f1f586b97ccfa5a4162b4dd09a68b5202fc373c95e0e315
    applied_at: 2026-11-20T21:26:28Z
  - op_id: mcp-done-008
    request_hash: sha256:4023fabda0964ae5b920ba3706af8391dc2f689e052a4475eaed55cc6c25b1a3
    applied_at: 2026-11-20T22:05:25Z
checkpoint:
  summary: "Landed in 7fc7066: async device tools (devices/scan/scan_status/ls/cleanup plan+execute) with --allow-cleanup gating; local scans run in background threads, remote sessions kept alive and reaped on EOF. 18 cli+mcp tests green; live ufo MCP session verified end-to-end (scan <1s, poll, ls with sizes, plan, execute trashed 8MiB with .trashinfo on remote fs)."
  reported_at: 2026-11-21T06:20:00Z
  reporter: agent
---
## Scope
`spacelens mcp` gains remote devices: a devices tool over ~/.ssh/config, device routing for discovery/app-audit, remote scan/cleanup-plan/cleanup-execute tools, and a local+remote cleanup execute gated behind `--allow-cleanup`.

## Acceptance criteria
- `spacelens mcp --allow-cleanup` lists the cleanup tools; without the flag they are absent.
- Remote tools connect through the shared runtime and reap the remote server on EOF.
- cargo test/clippy/fmt green; a live ufo MCP round-trip is recorded in tsk_remote_acceptance.
