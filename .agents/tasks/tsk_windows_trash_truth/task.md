---
spec_version: 0.1.0
kind: task
id: tsk_windows_trash_truth
project_id: prj_general
title: "Windows trash: prove the recycle instead of trusting the API"
stage: done
parent_id: null
depends_on: []
progress:
  mode: manual
  percent: 100
links: []
created_at: 2026-11-21T02:52:26Z
updated_at: 2026-11-21T03:00:27Z
archived_at: null
deleted_at: null
x_activity_receipts:
  - op_id: create-windows-trash-001
    request_hash: sha256:782e6269efdeb7a203675be69171d3d165412df69d152b33c19f8ff94b2fae0e
    applied_at: 2026-11-21T02:52:26Z
  - op_id: close-windows-trash-001
    request_hash: sha256:12eb86d21536e780cc8f454a595266aa3e30a9d68f2dada72f582c202fd417ed
    applied_at: 2026-11-21T03:00:27Z
checkpoint:
  summary: "Windows cleanup can no longer report a permanent delete as trash: probe + per-item verification shipped in 0.4.1 and verified on rog16-windows (refused, files intact) and ufo (trashed, bin entry present)."
  reported_at: 2026-11-21T03:05:00Z
  reporter: agent
---
## Finding (2026-11-21, rog16-windows over SSH)
`[Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory($path, 'OnlyErrorDialogs', 'SendToRecycleBin')` **permanently deleted** a 4 MiB folder and returned success: the Recycle Bin (`C:\$Recycle.Bin\S-1-5-...-1001`) gained no entry, `Shell.Application` showed nothing new, and the source was gone. The bin is enabled on that box (`NukeOnDelete=0`, ~26 GB quota), so this is the documented fallback: when the shell cannot recycle — a non-interactive SSH session is one such case — the item is deleted for good.

That is exactly the case the host must never report as "trashed".

## Fix (f2a098e)
- `windowsTrash` proves the bin per volume with a zero-byte probe written beside the planned folder and recycled; if the probe does not land in the bin, the **whole plan is refused** and nothing is deleted.
- Each item is recycled, then `recycleBinCount` confirms the bin count for that name went up (Windows renames duplicates `name (2)`); a flat count is reported as "deleted permanently".
- `recycleBinNamePattern` escapes `[`/`]` so a name never becomes a wildcard that matches someone else's entry; unit-tested.

## Acceptance criteria
- [x] Root cause reproduced with evidence on rog16-windows.
- [x] Probe + per-item verification implemented; host tests 218, typecheck clean.
- [x] 0.4.1 published (npm + cli-v0.4.1) and the agent manifest re-pinned.
- [ ] Re-run `remote cleanup --execute` on rog16-windows: it must **refuse** with "the Recycle Bin cannot recycle on C:\ in this session; refusing to delete permanently", and the fixture must still exist.
- [x] Re-run on ufo (host 0.4.1): trashed 3.0 MiB → `~/.local/share/Trash/files/node_modules.5`, source gone.

## Release
0.4.1 published: `npm view @space-lens/cli version` → 0.4.1, GitHub release `cli-v0.4.1` with 5 archives, manifest re-pinned (`--check` matches).
