# Disk discovery and developer cleanup UX

Researched 2026-10-02 from official product manuals and tool documentation. Product proposals below are recommendations, not claims about competitor implementations.

## Useful patterns from existing tools

- **CleanMyMac Space Lens:** pairs a size-based bubble map with a complete file list, links map hover to list highlighting, supports drill-down, Quick Look / reveal, checkboxes, and Review Selected before removal. Borrow the linked list and map, plus review flow; a particular chart shape is optional. [MacPaw Space Lens guide](https://macpaw.com/support/cleanmymac-x/knowledgebase/space-lens)
- **CleanMyMac large-file discovery:** groups results by kind, size, or access date, exposes preview and reveal, and leaves large files unselected because they can contain useful data. Borrow explicit selection and useful filtering. [MacPaw Large & Old Files guide](https://macpaw.com/support/cleanmymac/knowledgebase/large-and-old)
- **DaisyDisk:** stages files in an expandable Collector; files remain intact while staged, and users can preview or remove entries before committing. It prevents staging certain important system folders. Its actual Delete operation permanently removes files, with a short cancellation window. Borrow staging and inspection, while preserving Space Lens's own Trash contract. [DaisyDisk deletion guide](https://daisydiskapp.com/guide/4/en/DeletingFiles/)
- **WizTree:** uses a treemap to reveal large files and folders; File View supports name, size, and modified-date filters, with a result limit because rendering every file can be slow. Borrow a direct largest-files list with bounded rendering and simple filters. [WizTree overview](https://diskanalyzer.com/), [official search guide](https://diskanalyzer.com/guide#search)

## Recommended focused scope

Build one compact workspace with three useful views: **Browse**, **Large files**, and **Developer cleanup**. Keep the location, scan status, and selected count / bytes visible. A narrow sidebar, restrained type scale, dense readable rows, and an optional linked map make the interface attractive without consuming the working area.

1. **Browse:** size-sorted folders, breadcrumbs, search, readable paths, reveal / copy-path actions, and selection checkboxes. Selecting an item stages it; navigating and filtering retain the selection.
2. **Large files:** a flat descending-size list across the scanned scope, minimum-size and file-type filters, and a displayed result count. Default to no selection. Modified time can be added when the engine provides it; do not present it as last access time.
3. **Developer cleanup:** size-sorted groups for Rust build output, Node dependencies, Python caches, and other explicitly recognized tool caches. Show each candidate's project/path, size, reason, and regeneration consequence. Provide group selection and **Select visible** with an explicit count. All items remain unselected until the user chooses.
4. **Gitignored space:** show a total and a reviewable list under Developer cleanup, labeled as a classification rather than a safe-cleanup recommendation. It overlaps known cache groups, so never add it to those groups to produce a misleading grand total.
5. **Review selection:** one collector across views, deduplicated by path and parent/child containment. Show exact paths, distinct count, estimated bytes, removal mode, and individual removal controls. Request a fresh host plan and show its result before executing. Report successful and failed paths separately.

## What counts as a cache?

| Candidate | Meaning and consequence | Recommended treatment |
| --- | --- | --- |
| Rust `target` | Cargo build output defaults to workspace `target`; configurable target and build directories are supported. `cargo clean` removes generated artifacts. [Cargo build cache](https://doc.rust-lang.org/cargo/reference/build-cache.html), [cargo clean](https://doc.rust-lang.org/cargo/commands/cargo-clean.html) | Early scope. Require Cargo context or clear artifact markers before confidently labeling an arbitrary directory named `target`. Explain that rebuilding is required. |
| `node_modules` | Installed dependencies; `npm ci` requires a matching lockfile and replaces an existing `node_modules`. [npm ci](https://docs.npmjs.com/cli/v11/commands/npm-ci/) | Early scope, labeled **Dependencies**, not generic cache. Explain reinstall and possible download/build cost; preserve manifests and lockfiles. |
| Python `__pycache__` | Python automatically caches compiled modules and recompiles when needed. [Python module documentation](https://docs.python.org/3/tutorial/modules.html#compiled-python-files) | Early scope; bytecode cache, distinct from source and standalone compiled distributions. |
| `.pytest_cache` | Cross-session test state; pytest provides `--cache-clear`. [pytest cache guide](https://docs.pytest.org/en/stable/how-to/cache.html) | Early scope; explain loss of previous test-state hints. |
| `.mypy_cache`, `.ruff_cache` | Mypy caches type information for incremental checks; Ruff stores project cache results and exposes `ruff clean`. [mypy command line](https://mypy.readthedocs.io/en/stable/command_line.html#incremental-mode), [Ruff settings](https://docs.astral.sh/ruff/settings/#cache-dir), [Ruff commands](https://docs.astral.sh/ruff/configuration/#full-command-line-interface) | Early scope; recognized analysis caches, rebuilt by the tools. |
| pip download/wheel cache | pip exposes configured location and size via `pip cache dir` / `info`, and `purge` clears HTTP and wheel caches; caching avoids downloads/builds. [pip caching](https://pip.pypa.io/en/stable/topics/caching/) | Follow-up provider. Resolve configuration rather than assuming an OS path; explain slower subsequent installs. |
| npm package cache | npm describes this as a cache, with clearing useful for reclaiming space and verification able to collect unneeded data. [npm cache](https://docs.npmjs.com/cli/v11/commands/npm-cache/) | Follow-up provider with configured-location discovery; distinguish it from `node_modules`. |
| uv cache | uv explicitly says never to modify its cache directly; it provides `uv cache clean` and `uv cache prune`, with locking around cache modification. [uv caching](https://docs.astral.sh/uv/concepts/cache/#cache-safety) | Tool-managed provider only; do not pass a uv cache directory to generic recursive deletion. |
| `.venv` / `venv` | Contains an interpreter and installed libraries. Python calls environments disposable when recreation is available, but they are environments serving projects. [Python venv](https://docs.python.org/3/library/venv.html) | Separate **Environments** category in a later scope, manually reviewed; never classify as a Python cache. |
| Other ignored paths | Git ignore rules classify intentionally untracked files; already tracked files are unaffected. [gitignore](https://git-scm.com/docs/gitignore) | Review individually. Names such as `.env`, database files, datasets, uploads, and build exports do not prove regeneration is possible. |

Avoid indiscriminate cleanup of every directory named `cache`, every OS cache directory, Docker volumes, package stores, or browser profiles. Add providers once their ownership, configured location, and supported cleanup mechanism are understood.

## Repository constraints and implementation implications

- Current Rust cleanup presets are only Node, Rust, and Gitignored; Node/Rust recognition checks directory names. Python and global-cache providers are proposed additions. [Engine candidate implementation](../../vendors/kuntu/crates/kuntu-scan/src/clean.rs)
- The scan engine can preserve ignored-directory byte totals while collapsing their children (`ignored_mode: summarize`). Its ignore implementation loads `.gitignore` files encountered during traversal; it does not establish the Git index or all standard Git exclusions. Therefore label current totals **Matched by .gitignore** or document their scope. [Scan implementation](../../vendors/kuntu/crates/kuntu-scan/src/scanner.rs) Git's full standard exclusions also include repository and user-global rules. [git ls-files](https://git-scm.com/docs/git-ls-files#Documentation/git-ls-files.txt---exclude-standard)
- Gitignored is not a disposal signal. Git itself provides dry-run and interactive cleanup. [git clean](https://git-scm.com/docs/git-clean) Recommendations: never auto-select unknown ignored data, keep reviewed selection, and avoid touching nested repositories.
- The web/extension contract is Trash-only, and host/Tauri execution uses OS Trash. The generic Rust cleanup helper separately performs permanent removal; do not use it to bypass the frontend contract. [Cleanup contract](../../packages/contract/src/cleanup.ts), [host execution](../../packages/host/src/scan-store.ts), [Tauri execution](../desktop/src-tauri/src/engine.rs), [Rust cleanup helper](../../vendors/kuntu/crates/kuntu-scan/src/clean.rs)
- Use **Move to Trash** and **Bytes moved** for Trash outcomes. A selected-size estimate does not prove immediate free space: DaisyDisk documents that macOS snapshots can retain deleted storage. [DaisyDisk storage explanation](https://daisydiskapp.com/guide/4/en/DeletingFiles/)

## Platform appearance is secondary

Recommend a Settings panel with system/light/dark appearance and an optional platform-style preset (Automatic, macOS, Windows, neutral). Keep the dense information layout, selection model, keyboard access, contrast, and cleanup clarity consistent. Platform presets may adjust chrome, font, corners, and control treatment; they should not determine which functionality exists or dominate the main workspace. Prioritize compactness and usability before adding decorative styling options.

## DaisyDisk existing-result review — 2026-10-02

Used computer use on the running DaisyDisk app's existing Macintosh HD scan; did not start a new scan, grant permissions, preview files, collect/delete items, or enter cloud storage.

- Clicking the Macintosh HD breadcrumb returned from System/Library to the disk overview. The disk view separated measured categories from hidden space, free space and free + purgeable.
- Clicking hidden space opened an explanation and a dedicated breakdown (purgeable, snapshots, still hidden), with an explicit note that missing access limits the scan. These categories are not all additive: snapshots can be part of purgeable space.
- List highlight and chart highlight shared the same category color. Breadcrumbs make the current scope clear without a separate navigation mode.
- The persistent collector area gives a clear destination for review. Space Lens already keeps selection across views and reviews it before cleanup.

The subsequent full-disk work implemented explicit coverage reporting, local-volume boundaries and cloud exclusions. Cloud safety and size semantics are documented separately in [CLOUD_SCAN_RESEARCH.md](./CLOUD_SCAN_RESEARCH.md). A compact coverage strip opens a details panel separating measured allocated/logical bytes, system capacity, access denials and unknown excluded regions. APFS capacities are not added together. This is separate from the shared component fix below.

## Shared control fix — 2026-10-02

Installed Select and Checkbox source from the official shadcn-svelte Vega registry, adapting only workspace imports/icons and keeping existing Bits UI dependencies. Sorting, scanned-location choice, cache type and minimum-size choice use the shared Select composition. Search uses the existing shadcn Input. Product-specific navigation and appearance choices retain their application layout.

Browser verification on the explicitly allowed project folder: light/dark sort popup contrast, arrow padding, mouse and keyboard sorting, Escape focus restoration, closed-menu labels, category/size filters, and collector checkbox toggling. No full-disk or cloud scan was performed. Evidence: `preview/dark-shadcn-sort.png`, `preview/dark-shadcn-filters.png`.
