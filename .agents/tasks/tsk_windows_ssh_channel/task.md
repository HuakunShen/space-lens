---
spec_version: 0.1.0
kind: task
id: tsk_windows_ssh_channel
project_id: prj_general
title: Windows SSH channel in crates/remote
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
  - op_id: create-windows-002
    request_hash: sha256:5b359a39c1bc744221ff975f9d69c018f1a2eae6854a8d2ed5c910f84fde7836
    applied_at: 2026-11-20T21:26:28Z
  - op_id: windows-done-009
    request_hash: sha256:f795c8023117e81f8d7a46d743e94a5c89d9c3251f2a2d1a2d549b445db40efa
    applied_at: 2026-11-20T22:36:19Z
checkpoint:
  summary: "Live acceptance on rog16-windows complete: pwsh EncodedCommand probe (5.1-compatible after the (if..) expression fix), agent .exe via scp (stdin EOF never reaches the wrapped pwsh child), npm.ps1 install with PATH-prefixed node dir, serve/tunnel/pair, scan (96MiB media.bin + 24MiB node_modules correct), cleanup --execute -> Recycle Bin verified. Windows sessions drop the memory.stdio requirement and serve without --allow-memory."
  reported_at: 2026-11-21T07:05:00Z
  reporter: agent
---
## Scope
PowerShell EncodedCommand channel, windows probe script, layouts, install/serve/kill scripts, zip manifest+extraction, runtime windows branch. Delegated to a subagent (session ses_e05265864ffe30IlCB4mj9C44e); scope limited to crates/remote.

## Acceptance criteria
- crates/remote tests/clippy/fmt green; POSIX path unchanged (runtime_pipeline passes).
- Real acceptance on rog16-windows recorded in tsk_remote_acceptance.
