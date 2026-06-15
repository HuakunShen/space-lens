import { executeCleanup, planCleanup, scanDirectory, scanDirectoryWithProgress } from 'space-lens'

import type { CliOptions, DirectoryNode, PlanEntry, PlanLike } from './model.js'
import type { ScanProgressEvent } from 'space-lens'

export interface SpaceLensData {
  scanTrees: DirectoryNode[]
  plan: PlanLike
}

export interface CleanupOutcome {
  removed: PlanEntry[]
  bytesRemoved: number
  errors: string[]
}

export interface LoadSpaceLensOptions
  extends Pick<CliOptions, 'paths' | 'presets' | 'ignoreHidden'> {
  respectGitignore?: boolean
  ignoredMode?: string
}

export function loadSpaceLensData(
  options: LoadSpaceLensOptions,
): SpaceLensData {
  return {
    scanTrees: scanDirectory({
      directories: options.paths,
      ignoreHidden: options.ignoreHidden,
      fullPath: false,
      respectGitignore: options.respectGitignore ?? true,
      ignoredMode: options.ignoredMode ?? 'summarize',
    }),
    plan: planCleanup({
      directories: options.paths,
      presets: options.presets,
      ignoreHidden: options.ignoreHidden,
    }),
  }
}

export async function loadSpaceLensDataWithProgress(
  options: LoadSpaceLensOptions,
  onProgress: (event: ScanProgressEvent) => void,
): Promise<SpaceLensData> {
  const scanTrees = await scanDirectoryWithProgress(
    {
      directories: options.paths,
      ignoreHidden: options.ignoreHidden,
      fullPath: false,
      respectGitignore: options.respectGitignore ?? true,
      ignoredMode: options.ignoredMode ?? 'summarize',
    },
    onProgress,
  )

  return {
    scanTrees,
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
