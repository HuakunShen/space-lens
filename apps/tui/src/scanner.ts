import { executeCleanup, planCleanup, scanDirectory } from 'space-lens'

import type { CliOptions, DirectoryNode, PlanEntry, PlanLike } from './model.js'

export interface SpaceLensData {
  scanTrees: DirectoryNode[]
  plan: PlanLike
}

export interface CleanupOutcome {
  removed: PlanEntry[]
  bytesRemoved: number
  errors: string[]
}

export function loadSpaceLensData(options: Pick<CliOptions, 'paths' | 'presets' | 'ignoreHidden'>): SpaceLensData {
  return {
    scanTrees: scanDirectory({
      directories: options.paths,
      ignoreHidden: options.ignoreHidden,
      fullPath: false,
      respectGitignore: true,
      ignoredMode: 'summarize',
    }),
    plan: planCleanup({
      directories: options.paths,
      presets: options.presets,
      ignoreHidden: options.ignoreHidden,
    }),
  }
}

export function executeCleanupEntries(entries: PlanEntry[]): CleanupOutcome {
  const totalSize = entries.reduce((total, entry) => total + entry.size, 0)

  return executeCleanup({
    entries,
    totalSize,
    errors: [],
  })
}
