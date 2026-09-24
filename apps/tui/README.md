# Space Lens TUI

The published `@space-lens/cli` package uses Solid and the public `@uniview/tui-solid` binding.

This is the current `main` branch TUI. It runs on Node.js 20+, and currently covers the existing scan/cleanup workflow; the new Rust iCloud eviction API is exposed through NAPI but is not wired into this TUI yet.

```bash
yarn tui . --preset node
# or, from this package directory
vite-node src/cli.ts . --preset node
```

## Serve (web workbench over HTTP)

`spacelens serve` starts an authenticated HTTP host (built on
`@space-lens/host`) that serves the web UI and speaks the closed JSON contract
(`@space-lens/contract`): pairing tickets, bearer sessions, REST reads, and SSE
scan events.

```bash
spacelens serve                       # loopback only, read-only, current dir
spacelens serve --allow-cleanup       # also grant trash-based cleanup
spacelens serve --host 0.0.0.0 --allow-cidr 192.168.1.0/24
spacelens serve --host en0 --allow-lan
spacelens serve --ui-origin https://<your>.workers.dev   # needs SPACLENS_HOSTED_PASSWORD
spacelens serve --machine             # supervisor mode: readiness JSON on stdout, ticket on stderr
```

Rules that hold in every mode:

- Pairing tickets are 256-bit, single-use, 60 seconds, and minted only on the
  serving terminal — never over HTTP.
- Cleanup through the web is trash-only. Permanent deletion stays a CLI/TUI
  `--execute` concern.
- Scans may only target `--root` directories (default: the working directory).

## Publish

Build and publish the public CLI from this directory:

```bash
yarn build
npm publish --access public
```

The package uses `@uniview/tui-solid` and requires Node.js 20 or newer.

## Controls

In Scan mode, use `j/k` or arrows to move and `enter` to expand or collapse
the active folder. Click a row to focus it; clicking a folder also toggles its
expansion. Preset candidates such as `node_modules` and `target` are shown in
red. Press `d` followed by `enter` to delete the current file or directory, or
`A` followed by `enter` to delete all preset candidates at once. In Clean mode,
use `space` to select, `a` to select all, and `x` followed by `enter` to delete.
`tab` switches modes; `esc`, `q`, and Ctrl-C cancel or exit.
