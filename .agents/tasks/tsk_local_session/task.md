---
spec_version: 0.1.0
kind: task
id: tsk_local_session
project_id: prj_general
title: Ticket-free loopback sessions for the web workbench
stage: done
parent_id: null
depends_on: []
progress:
  mode: manual
  percent: 100
links: []
created_at: 2026-11-21T04:49:17Z
updated_at: 2026-11-21T04:49:17Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-local-session-001
    request_hash: sha256:15c2457f732a22ea310ab54b5f06d15557872405e1ab79cedb71eb2b72fbb232
    applied_at: 2026-11-21T04:49:17Z
---
## Why
Opening the workbench meant copying a fresh single-use pairing ticket every time: the token lived in sessionStorage (per tab) and the only ticket-free route was the development-only one. Owner asked to stop requiring a code, or remove the feature until it was pleasant.

## What shipped (0.4.2)
- `POST /api/v1/session/local` mints a session for a **loopback peer**, on by default for a loopback listener with no hosted password; `--no-local-session` turns it off; hosted or wider binds keep the ticket.
- The web app stores the token in **localStorage** and, with no ticket/token (or a stale one), silently opens a local session before showing the pairing form.
- `serve` prints `open this machine without a ticket: http://127.0.0.1:<port>/`; `remote open` prints the plain tunnel URL with the one-time ticket as fallback.

## Evidence
```
local serve  GET http://127.0.0.1:9470/ (no ?pair)  → workbench ready, localStorage spacelens.session.token set
reload                                            → ready, no pairing form
second tab                                        → ready, no pairing form
poisoned token → reload                           → silently re-minted, ready
ufo tunnel   http://127.0.0.1:64654/ → 200; POST /api/v1/session/local → {"token":"sls_…"}
host tests   28 files / 223 tests (5 new: loopback v4/v6/v4-mapped, non-loopback 403,
             --no-local-session 503, hosted password off by default, non-loopback bind refused)
```
No cookie is involved; the bearer token is the same one the ticket exchange would have returned.
