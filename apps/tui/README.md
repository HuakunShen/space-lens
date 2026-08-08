# Space Lens TUI

The published `@space-lens/cli` package uses Solid and the public `@uniview/tui-solid` binding.

```bash
yarn tui . --preset node
# or, from this package directory
vite-node src/cli.ts . --preset node
```

## Publish

Build and publish the public CLI from this directory:

```bash
yarn build
npm publish --access public
```

The package uses `@uniview/tui-solid` and requires Node.js 20 or newer.

## Controls

In Scan mode, use `j/k` or arrows to move and `enter` to expand or collapse
the active folder. In Clean mode, use `space` to select, `a` to select all,
and `x` followed by `enter` to delete. `tab` switches modes; `esc`, `q`, and
Ctrl-C cancel or exit.
