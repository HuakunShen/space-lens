---
spec_version: 0.1.0
kind: task
id: tsk_remote_acceptance
project_id: prj_general
title: "Real-host remote acceptance: ufo, mac-mini, rog16"
stage: done
parent_id: null
depends_on: []
progress:
  mode: manual
  percent: 100
links: []
created_at: 2026-11-20T21:26:28Z
updated_at: 2026-11-20T22:36:19Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-acceptance-003
    request_hash: sha256:a879a2536f5c431b5fc1a24f78dfcecd60df2f83a712be525cb27e2f51b5c742
    applied_at: 2026-11-20T21:26:28Z
  - op_id: acceptance-ufo-006
    request_hash: sha256:61964e86bbee801cdee07334d53fee19751f0ff0d16a180ec77d1b29df0103f7
    applied_at: 2026-11-20T21:39:05Z
  - op_id: acceptance-done-010
    request_hash: sha256:cf5e5924da7d6d474954cf3ffcaa458435272d488efd27ed66bb777fdbc8120d
    applied_at: 2026-11-20T22:36:19Z
checkpoint:
  summary: "All three real hosts verified: ufo (scan/discovery/cleanup trash/MCP session/browser pairing), mac-mini (scan correct after nvm probe fallback + npm PATH prefix), rog16-windows (scan + cleanup execute to Recycle Bin). Host fixes en route: Host authority header, npm env-node shebang, publishable deps, gio-less freedesktop trash, windows roots/engine file URLs, bundled web UI."
  reported_at: 2026-11-21T07:06:00Z
  reporter: agent
---
## Scope
End-to-end with local artifacts (env overrides) then released artifacts: scan/discovery/cleanup plan+execute/memory, SSE, disconnect pid reaping, MCP round-trip, desktop device connect.

## Acceptance criteria
- ufo (Linux x86_64), mac-mini (macOS arm64), rog16-windows (Win11): connect, scan, print top entries, cleanup dry-run; trash execute where the host trash backend exists.
- Findings fixed or filed: gio absent on headless Linux (host trash fallback task), node via nvm probe fallback (done).
