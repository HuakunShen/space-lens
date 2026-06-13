# spacelens

OpenTUI-powered terminal viewer for Space Lens cleanup plans.

The command-line surface is defined with Effect CLI, and the startup workflow is
run as an Effect program before handing control to OpenTUI.

It has two modes:

- `scan`: inspect disk usage as a tree.
- `clean`: inspect cleanup candidates, select paths, and execute deletion after
  confirmation.

## Runtime

OpenTUI `0.4.x` currently needs Bun for its native FFI runtime. Use the root
`yarn tui` script or run the app with `bun`.

Running the built CLI with `node` prints a runtime error instead of opening the
TUI.

## Run From The Repo Root

Use this for normal local scanning:

```bash
yarn tui ~/Dev --preset rust
yarn tui ~/Dev --preset node,gitignored --sort path
yarn tui . --preset rust
npx @space-lens/cli ~/Dev --preset rust
```

`yarn tui` intentionally runs `bun apps/tui/src/cli.ts` from the current working
directory, so relative paths like `.` mean the directory where you invoked the
command.

## Controls

```text
tab              Switch scan/clean mode
j / down         Move down
k / up           Move up
space            Toggle the active cleanup candidate in clean mode
a                Select or unselect all visible cleanup candidates
x                Ask to delete selected cleanup candidates
enter            Confirm deletion after pressing x
esc              Cancel deletion confirmation
q / ctrl+c       Quit
```

Deletion is only available from clean mode, only for selected rows, and only
after the `x` then `enter` confirmation flow. After deletion, the TUI refreshes
both the scan tree and cleanup plan.

## Run From The App Workspace

Useful while developing the TUI package itself:

```bash
yarn workspace @space-lens/cli dev -- --preset rust .
```

This runs with `apps/tui` as the working directory, so `.` means `apps/tui`, not
the repository root.

## Options

```text
-p, --preset <name>       Cleanup preset: node, rust, gitignored. Repeat or comma-separate.
    --ignore-hidden       Skip hidden paths while searching.
    --sort <mode>         Sort rows by size or path. Defaults to size.
-h, --help                Show help.
```

Press `Ctrl+C` to exit the TUI.

## Build And Verify

```bash
yarn workspace @space-lens/cli build
yarn workspace @space-lens/cli test
yarn workspace @space-lens/cli typecheck
```
