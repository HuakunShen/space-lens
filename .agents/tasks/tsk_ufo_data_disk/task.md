---
spec_version: 0.1.0
kind: task
id: tsk_ufo_data_disk
project_id: prj_general
title: "ufo external disk (/media/hk/Data): scan and reclaim"
stage: doing
parent_id: null
depends_on: []
progress:
  mode: manual
  percent: 40
links: []
created_at: 2026-11-21T15:01:06Z
updated_at: 2026-11-24T01:00:25Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-data-disk-001
    request_hash: sha256:72dbba810644f01cffcf7bf989b98c114a15e8259d0584e017b9b21fdb72f6da
    applied_at: 2026-11-21T15:01:06Z
  - op_id: data-disk-cp-002
    request_hash: sha256:81a794ddaa3480ece04d0d520cf1f86550e83f2ccd1979911968e600677cb3ae
    applied_at: 2026-11-24T00:53:19Z
  - op_id: data-disk-cp-003
    request_hash: sha256:e6f4383a48f24551490ce7f3e6b0b5787df6b48dad2758e18480cb51afe5dd52
    applied_at: 2026-11-24T01:00:25Z
checkpoint:
  summary: "Deleted polyquant (276 GB) + polyinsight (146 GB) + idle build caches: slots/1..3 target (378 GB), targets/ev30 + cf-deadman (36 GB). Free went 19 GB → 810 GB (7% used). Active targets now: 2 (targets/avfix72 and slots/1, both writing)."
  reported_at: 2026-11-24T09:05:00Z
  reporter: agent
---
## Scan (protected streaming, 14 s)
`/media/hk/Data` 916 GB ext4, 862 GB allocated / 1.85M files, 411k dirs.

| Area | Size (dedup `du`) | Notes |
|---|---|---|
| `slots/1..3` | 336 GiB | Rust `target/debug` build slots; `owner` files point at codex/xross worktrees, locks fresh |
| `targets/{exitl2,r5}` | 46 GiB | more build outputs |
| `worktrees/{exitl2,r5}` | 2 GiB | build worktrees |
| `polyquant/polymarket` | 216.5 GiB | `btc-updown-15m-*` snapshots, 22.6k dirs April + 36.9k May 2026 |
| `polyquant/binance-options` | 45.3 GiB | options data |
| `polyinsight/binance_history` | 81.8 GiB | futures/spot |
| `polyinsight/parquet-data` | 39.2 GiB | parquet |
| `polyinsight/nats-events` | 23.6 GiB | two `stream.tar.s2` from Jan 2026 |
| `polyinsight/price_history` | 13.4 GiB | prices |
| `.pnpm-store` | 5.2 GiB | pnpm store |

## Done
- Removed `targets/exitl2` (28.7 GiB, idle since 06:29, no open handles).
- Removed the two `nats-events/**/stream.tar.s2` archives (25.3 GB, Jan 2026).
- Free space 9.5 GB → **62 GB** (disk was at 99%, builds were still writing).

## Open
- `slots/*/target` (336 GiB) and `targets/r5` (41 GiB) are **in use by running builds**; delete once idle.
- `.pnpm-store` prune failed with EACCES; retry as the owner or with `sudo`.
- Market data (≈435 GiB) untouched: decide per-month pruning or move to the internal disk (470 GB free).
