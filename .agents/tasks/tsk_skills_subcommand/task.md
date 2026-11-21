---
spec_version: 0.1.0
kind: task
id: tsk_skills_subcommand
project_id: prj_general
title: "spacelens skills: install/update the repo skill via npx skills"
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
  - op_id: create-skills-001
    request_hash: sha256:1d48cc9d4471865725ee700b7337964cfe5598b6aa04030493a420b6a1023bad
    applied_at: 2026-11-21T04:49:17Z
---
## Why
Owner wants `spacelens skills install` to call `npx skills` for this repository's skill so updating is one command.

## What shipped (0.4.3)
`spacelens skills install | update | list | remove` map onto `npx -y skills@latest …` with the repo (`HuakunShen/space-lens`) and skill (`space-lens`) filled in: install is user-level by default (`--project`, `--agent <name>` repeatable, `--copy`, `--yes`), update/list expose the CLI's scope flags, remove cleans both.

## Evidence
```
spacelens skills install --yes
  → ~/.agents/skills/space-lens/SKILL.md, registered for Antigravity, Antigravity CLI,
    Codex, Gemini CLI, GitHub Copilot +3 more  (Source: HuakunShen/space-lens)
  → one agent (PromptScript) refuses *global* installs; the wrapper reports it and points
    at --agent / --project
spacelens skills update --yes     → "All global skills are up to date"
spacelens skills list             → space-lens  ~/.agents/skills/space-lens
cargo test -p spacelens --bin spacelens → 15 passed (4 new argv tests)
```
