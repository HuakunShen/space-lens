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
updated_at: 2026-11-21T12:59:57Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-ufo-cleanup-001
    request_hash: sha256:fe7397adc2117352dc5ecd02c103e25c019c91ee3cc44fe11158258c43ecdc53
    applied_at: 2026-11-21T07:41:10Z
  - op_id: update-ufo-cleanup-002
    request_hash: sha256:f69256329894708029e17886cff7e0fe4e473649514cf17281fce4dbaa0f96bf
    applied_at: 2026-11-21T12:59:57Z
checkpoint:
  summary: Trash emptied; CrossCopy and videos removed with full-history bundles as backups; ~/dev 169.7 GiB → ~31–38 GiB; disk 457 GB free (from 367). Only root-owned stragglers (1.6 GiB) need the owner's sudo.
  reported_at: 2026-11-21T21:10:00Z
  reporter: agent
---
## Scope
Owner asked for an analysis of ufo's `~/dev` (182 GB / 5.05M files), then approved tier A, then B+C, then emptying the trash and removing `CrossCopy` (and earlier `videos`).

## Executed
- **A**: 13 Python environments + 467 `node_modules` (67 batches) → 51.4 + 27.6 GiB.
- **B+C**: `videos/projects/*/{out,audio}`, `polyquant-v2/research/*/cache`, `quant/output/research/*`, CosyVoice models, 2048AI models, ts2vec datasets → 42.05 GiB.
- **Trash emptied** (`rm -rf ~/.local/share/Trash/{files,info}/*`) → +61 GiB free.
- **CrossCopy removed** after `git bundle create --all` (38 MB, verified complete; copy at `/tmp/crosscopy-backup-2026-11-21.bundle` locally and `~/crosscopy-backup-2026-11-21.bundle` on ufo) and saving the one dirty file (`pnpm-lock.yaml`). Its sibling worktree `CrossCopy-tui-enrollment-dev` (which pointed into the deleted `.git`) was re-initialised as a standalone repo so its 8k files stay usable.
- **videos removed** after `git bundle create --all` (4.48 GB, `~/videos-backup-2026-11-21.bundle`) and copying the two untracked `script.md` dirs to `~/videos-untracked-backup/`.

## Result
```
~/dev         182,208,434,176 B  →  33,314,711,244 B   (169.7 GiB → 31.0 GiB)
disk free     367 GB  →  457 GB
```

## Left behind
- `~/dev/CrossCopy` 1.6 GiB of **root-owned** files (docker/swift build caches) — `rm` is refused; needs `sudo rm -rf ~/dev/CrossCopy`. Same for `~/dev/multi-network-interface-proxy/logs`.
- `polyquant-v2` 13 GiB (market data + `polymarket.db`), `others` 4.4, `CrossCopy-crossflow` 3.7, `data-intensive-app-tts` 1.3 — sources/data, untouched.
- Backups: `~/videos-backup-2026-11-21.bundle`, `~/crosscopy-backup-2026-11-21.bundle`, `~/crosscopy-tui-pnpm-lock.yaml.bak`, `~/videos-untracked-backup/`.
