---
spec_version: 0.1.0
kind: task
id: tsk_ufo_dev_cleanup
project_id: prj_general
title: "ufo ~/dev cleanup: venvs, node_modules, render outputs, models"
stage: done
parent_id: null
depends_on: []
progress:
  mode: manual
  percent: 100
links: []
created_at: 2026-11-21T07:41:10Z
updated_at: 2026-11-21T07:41:10Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-ufo-cleanup-001
    request_hash: sha256:fe7397adc2117352dc5ecd02c103e25c019c91ee3cc44fe11158258c43ecdc53
    applied_at: 2026-11-21T07:41:10Z
---
## Scope
Owner asked for an analysis of ufo's `~/dev` (182 GB / 5.05M files) and then approved tier A, then B+C, for trash-only cleanup.

## Executed (all into `~/.local/share/Trash`, recoverable)
- **A**: 13 Python environments (`.venv`/`.venvs`, incl. `videos/.venvs` 22.2 GiB, `quant/.../ml_env/py312_torch` 7.1) → 51.4 GiB; 467 `node_modules` in 67 batches → 27.6 GiB.
- **B**: `videos/projects/*/{out,audio}`, `polyquant-v2/research/*/cache`, `quant/output/research/*` (42 dirs) → part of 42.05 GiB.
- **C**: `others/CosyVoice/pretrained_models` 13.6, `2048AI/models` 1.2, `ts2vec/datasets` 0.8.

## Result
```
~/dev   182,208,434,176 B → 63,694,852,817 B   (169.7 GiB → 59.3 GiB, −110.4 GiB)
trash   114,266,687,442 B in 542 entries (each with .trashinfo)
```
Space returns when the trash is emptied (`rm -rf ~/.local/share/Trash/{files,info}/*`).

## Left alone
- `videos` and `CrossCopy` as whole repositories: **videos has 30 unpushed commits + 2 dirty paths**, **CrossCopy has 95 unpushed commits** and three worktrees. Deleting them would destroy that history; awaiting an owner decision (push first, artifacts-only, or delete anyway).
- `polyquant-v2/data` (market data + `polymarket.db`), `videos/.git`, sources.
- 14 nested `node_modules` (~0.3 GB) inside worktrees/vendored packages.

## Tool note
`spacelens remote trash` (new, unreleased) does the work; the host keeps 8 scan sessions, so at most seven parent folders per invocation.
