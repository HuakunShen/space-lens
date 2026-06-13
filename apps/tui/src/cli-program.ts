import { Args, Command, Options } from '@effect/cli'
import * as NodeContext from '@effect/platform-node/NodeContext'
import * as NodeRuntime from '@effect/platform-node/NodeRuntime'
import { Effect } from 'effect'

import { normalizeCliConfig, SORT_MODES } from './cli-config.js'
import type { CliOptions } from './model.js'
import { assertSupportedRuntime } from './runtime.js'

const APP_VERSION = '0.2.0'

const commandConfig = {
  paths: Args.text({ name: 'path' }).pipe(
    Args.atLeast(0),
    Args.withDescription('Directory to scan. Repeat by passing multiple paths.'),
  ),
  presets: Options.text('preset').pipe(
    Options.withAlias('p'),
    Options.repeated,
    Options.withDescription('Cleanup preset to include: node, rust, gitignored. Can be repeated or comma-separated.'),
  ),
  ignoreHidden: Options.boolean('ignore-hidden').pipe(Options.withDescription('Skip hidden paths while scanning.')),
  sort: Options.choice('sort', SORT_MODES).pipe(
    Options.withDefault('size'),
    Options.withDescription('Sort cleanup candidates by size or path.'),
  ),
}

export const spaceLensTuiCommand = Command.make('spacelens', commandConfig, (rawConfig) =>
  Effect.gen(function* () {
    const options = yield* Effect.try({
      try: () => normalizeCliConfig(rawConfig),
      catch: toError,
    })

    yield* runTuiProgram(options)
  }).pipe(Effect.catchAll(reportCliError)),
).pipe(Command.withDescription('OpenTUI scanner and cleanup picker for Space Lens.'))

const cli = Command.run(spaceLensTuiCommand, {
  name: 'Space Lens TUI',
  version: APP_VERSION,
})

export function makeCliEffect(argv: readonly string[]) {
  return cli(argv).pipe(Effect.provide(NodeContext.layer))
}

export function runCli(argv: readonly string[] = process.argv): void {
  NodeRuntime.runMain(makeCliEffect(argv))
}

const runTuiProgram = Effect.fn('runTuiProgram')((options: CliOptions) =>
  Effect.gen(function* () {
    yield* Effect.try({
      try: () => assertSupportedRuntime(),
      catch: toError,
    })

    const [{ executeCleanupEntries, loadSpaceLensData }, { runTui }] = yield* Effect.tryPromise({
      try: () => Promise.all([import('./scanner.js'), import('./ui.js')]),
      catch: toError,
    })

    const initialData = yield* Effect.try({
      try: () => loadSpaceLensData(options),
      catch: toError,
    })

    yield* Effect.tryPromise({
      try: () =>
        runTui({
          initialData,
          sort: options.sort,
          refreshData: () => loadSpaceLensData(options),
          executeEntries: executeCleanupEntries,
        }),
      catch: toError,
    })
  }),
)

function reportCliError(error: Error) {
  return Effect.sync(() => {
    process.stderr.write(`spacelens: ${error.message}\n`)
    process.exitCode = 1
  })
}

function toError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error))
}
