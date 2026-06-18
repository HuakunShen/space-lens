import * as spaceLens from 'space-lens'

import type { CliOptions, DirectoryNode, PlanEntry, PlanLike } from './model.js'
import type { DirectoryScanOptions, ScanProgressEvent } from 'space-lens'

const { executeCleanup, planCleanup, scanDirectory } = spaceLens

type ProgressScanner = (
  options: DirectoryScanOptions,
  onProgress: (event: ScanProgressEvent) => void,
) => Promise<DirectoryNode[]>

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
  const scanWithProgress = resolveProgressScanner()
  if (!scanWithProgress) {
    throw new Error(
      'The bundled Space Lens native scanner does not export scanDirectoryWithProgress. Rebuild the native scanner artifact before scanning in Kunkun.',
    )
  }
  const scanOptions = {
    directories: options.paths,
    ignoreHidden: options.ignoreHidden,
    fullPath: false,
    respectGitignore: options.respectGitignore ?? true,
    ignoredMode: options.ignoredMode ?? 'summarize',
  }
  const scanTrees = await scanWithProgress(scanOptions, onProgress)

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

function resolveProgressScanner(): ProgressScanner | null {
  const candidate = (spaceLens as Partial<{ scanDirectoryWithProgress: unknown }>).scanDirectoryWithProgress
  return typeof candidate === 'function' ? (candidate as ProgressScanner) : null
}
