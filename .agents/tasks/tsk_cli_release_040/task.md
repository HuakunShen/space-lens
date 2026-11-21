---
spec_version: 0.1.0
kind: task
id: tsk_cli_release_040
project_id: prj_general
title: Publish cli-v0.4.0 and pin the agent manifest
stage: todo
parent_id: null
depends_on: []
progress:
  mode: manual
  percent: null
links: []
created_at: 2026-11-20T21:26:28Z
updated_at: 2026-11-20T21:26:28Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-release-005
    request_hash: sha256:5bafc9ecc3a64b3100ed1affe5311b0413603065d69ecaf966f5a8f4bda04a6d
    applied_at: 2026-11-20T21:26:28Z
---
## Scope
Commit remaining work, tag cli-v0.4.0, push (cli-release.yml builds 5 archives + SHA256SUMS; npm-cli.yml publishes @space-lens/cli@0.4.0), download assets, `node scripts/agent-manifest.ts --dist`, commit the pinned manifest, re-verify the desktop build embeds it. Desktop app-v* build follows.

## Acceptance criteria
- Manifest carries real sha256/size for all pinned targets; provisioning resolves without env overrides; acceptance rerun against released artifacts.
