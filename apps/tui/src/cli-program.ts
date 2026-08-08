import { Args, Command, Options } from '@effect/cli'
import * as NodeContext from '@effect/platform-node/NodeContext'
import * as NodeRuntime from '@effect/platform-node/NodeRuntime'
import { Effect } from 'effect'

import { normalizeCliConfig, SORT_MODES } from './cli-config.js'
import type { CliOptions } from './model.js'
import { runTui } from './ui.js'

const APP_VERSION = '0.2.4'

const command = Command.make(
  'spacelens',
  {
    paths: Args.text({ name: 'path' }).pipe(Args.atLeast(0), Args.withDescription('Directory to scan.')),
    presets: Options.text('preset').pipe(
      Options.withAlias('p'),
      Options.repeated,
      Options.withDescription('Cleanup preset: node, rust, gitignored.'),
    ),
    ignoreHidden: Options.boolean('ignore-hidden').pipe(Options.withDescription('Skip hidden paths.')),
    sort: Options.choice('sort', SORT_MODES).pipe(
      Options.withDefault('size'),
      Options.withDescription('Sort by size or path.'),
    ),
  },
  (rawConfig) =>
    Effect.gen(function* () {
      const options = yield* Effect.try({
        try: () => normalizeCliConfig(rawConfig),
        catch: toError,
      })
      yield* runTuiProgram(options)
    }).pipe(Effect.catchAll(reportCliError)),
).pipe(Command.withDescription('Space Lens scanner and cleanup picker.'))

const cli = Command.run(command, { name: 'Space Lens TUI', version: APP_VERSION })

export function runCli(argv: readonly string[] = process.argv): void {
  NodeRuntime.runMain(cli(argv).pipe(Effect.provide(NodeContext.layer)))
}

const runTuiProgram = Effect.fn('runTuiProgram')((options: CliOptions) =>
  Effect.gen(function* () {
    const [{ executeCleanupEntries, loadSpaceLensData }] = yield* Effect.tryPromise({
      try: () => Promise.all([import('./scanner.js')]),
      catch: toError,
    })
    const initialData = yield* Effect.try({ try: () => loadSpaceLensData(options), catch: toError })
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
