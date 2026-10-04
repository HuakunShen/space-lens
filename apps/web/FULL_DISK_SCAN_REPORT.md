# Macintosh HD local-only heavy scan

Date: 2026-10-02. Scope: `/` and `/System/Volumes/Data`, each treated as an explicit local root. Cloud inspection, external disks, network mounts, cleanup and permission grants are excluded. The browser workbench runs on a loopback-only host.

The release Rust scanner installs macOS dataless-materialization opt-out before I/O on every scanning thread, resolves roots and descendants through pinned directory descriptors without following symlinks, and excludes known cloud paths before metadata. Whole-disk scans disable `.gitignore` content reads. Denials and unknown regions are reported separately from measured allocated/logical bytes. This does not control independent background provider activity or make APFS shared blocks reclaimable.

## Initial stress-run failure and repair

The first real run reached 1,887,862 entries and 128,119,922,688 measured allocated bytes in its last progress update. The HTTP host rejected the final report because a single JSON line exceeded its 512 MiB protocol bound. These progress counts are incomplete, not a successful disk total. No scanner process remained after termination. Sampled host RSS reached 760,152,064 bytes.

The CLI and host now use `--stream-nodes`: bounded per-node NDJSON, parent before child, followed by coverage/volume metadata. The host assembles the tree incrementally and accepts it only after a valid final message and zero process exit. This avoids a giant JSON string and a second whole-tree schema clone. Old full-report protocol remains compatible for existing fixtures.

## Successful repeated run

Scan `scan_5d721aea908e` reached ready on 2026-10-02 at 09:33:39 UTC. The release scanner measured:

| Measurement | Result |
| --- | --- |
| Allocated bytes measured | 128,232,116,224 B · 119.43 GiB |
| Logical file lengths | 146,415,435,479 B · 136.36 GiB |
| Files / directories | 1,432,965 / 413,416 |
| Rust traversal time | 12.685 s |
| Skipped entries / access denials | 66,771 / 750 |
| Issue samples | 200 of 66,771; full count retained |
| Sampled peak scanner RSS | 795,705,344 B · 0.74 GiB |
| Sampled peak host RSS during scan | 1,632,321,536 B · 1.52 GiB |

Memory was sampled once per second and can miss short peaks. The traversal time excludes subsequent streamed output, validation, indexing and rendering; it is not an end-to-end wall-clock benchmark. Host RSS after building discovery was approximately 2.02 GiB. The normal Node heap limit was retained.

The filesystem reported 994,662,584,320 B capacity and 643,092,684,800 B available at scan time. These values include storage outside the measured file tree; the difference is not a computed cloud size, reclaimable amount or exact hidden-space category. Binary display values now use KiB/MiB/GiB labels.

Actual macOS metadata confirmed `/Users` and `/System/Volumes/Data/Users` have the same device/inode. The scanner counted these directories once. The Data alternate view contains alias notices rather than another copy of their sizes; the UI labels them “Counted elsewhere.” Its small unique local subtree remains available. The underlying filesystem capacity is reported once.

Browser checks on the successful real report:

- Root totals, paged children, breadcrumbs and System/Data navigation remain usable; no console errors observed.
- Reload restored the same scan and selected location without starting a new filesystem scan.
- Real `Library/CloudStorage` and `Library/Mobile Documents` appear as `cloud-root`, Unknown, with entry/selection disabled. Their contents were not enumerated by this scanner; no cloud permissions were granted.
- Large-file discovery returned 1,476 files above 10 MiB; developer discovery returned 621 candidates. Both reused the protected report, without a second filesystem traversal.
- Whole-disk gitignore classification is disabled because this scan did not read ignore-rule contents. Local project scans support that classification separately.
- Protected cleanup is read-only. No user files were moved, evicted, downloaded or deleted by this run.

Evidence: [Full-disk overview](preview/local-full-disk-dark.png), [Coverage details](preview/local-full-disk-coverage.png), [dark shadcn sort](preview/local-protected-dark-sort.png).

## Verification

Pinned-FD safety review found no remaining source blocker. Rust workspace, streaming CLI fixtures, HTTP/contract/client, native desktop, UI helper tests and repository typechecks passed. Web and desktop frontend builds passed sequentially. See the implementation ledger in `docs/superpowers/plans/2026-10-02-local-full-disk-scan.md` for counts and protocol regression cases.

Cloud inspection remains deferred. Legacy generic scanner/GPUI routes do not inherit the new protected-mode guarantees. APFS snapshots/shared extents, inaccessible areas and provider background activity remain outside a claim of exact reclaimable disk usage or universal no-download behavior.
