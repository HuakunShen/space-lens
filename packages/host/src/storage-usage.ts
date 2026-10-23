// APFS accounting for a finished scan ("hidden space"). The release CLI reads
// the container through `diskutil` and Foundation; the host only validates
// what comes back. Every failure is "no breakdown": the scan itself is done.
import { execFile } from 'node:child_process'

import { StorageBreakdownSchema, type StorageBreakdown } from '@space-lens/contract'

const TIMEOUT_MS = 15_000
const MAX_STDOUT_BYTES = 1024 * 1024

export interface ScannedRoot {
  path: string
  bytes: number
}

export type CliRunner = (binary: string, args: readonly string[]) => Promise<string | null>

const runCli: CliRunner = (binary, args) =>
  new Promise((resolve) => {
    execFile(
      binary,
      [...args],
      { timeout: TIMEOUT_MS, maxBuffer: MAX_STDOUT_BYTES, windowsHide: true },
      (error, stdout) => resolve(error ? null : stdout),
    )
  })

export function storageUsageArgs(roots: readonly ScannedRoot[]): string[] {
  return ['storage-usage', ...roots.flatMap((root) => ['--root', root.path, '--scanned-bytes', String(root.bytes)])]
}

/** `undefined` off macOS, without roots, or when the CLI gives no valid breakdown. */
export async function readStorageUsage(
  binary: string,
  roots: readonly ScannedRoot[],
  run: CliRunner = runCli,
  platform: NodeJS.Platform = process.platform,
): Promise<StorageBreakdown | undefined> {
  if (platform !== 'darwin' || roots.length === 0) return undefined
  const stdout = await run(binary, storageUsageArgs(roots))
  if (stdout === null) return undefined
  let parsed: unknown
  try {
    parsed = JSON.parse(stdout)
  } catch {
    return undefined
  }
  const result = StorageBreakdownSchema.safeParse(parsed)
  return result.success ? result.data : undefined
}
