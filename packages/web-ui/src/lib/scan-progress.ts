// Determinate scan progress. The host reports what it has measured so far but
// not how much is left, so the total comes from a baseline: the last finished
// scan of the same location, or, for a whole-volume scan with no history, the
// volume's used space. Pure, so the arithmetic is testable.
import type { ScanVolume } from '@space-lens/contract'

/** What a finished scan left behind for the next one's estimate. */
export interface ScanSizeRecord {
  bytes: number
  entries: number
  at: string
}

export interface ScanBaseline {
  bytes: number
  /** Item count of the last scan; `null` when only disk capacity is known. */
  entries: number | null
  source: 'last-scan' | 'disk-used'
}

export interface ScanProgressEstimate {
  /** 0–0.99 while scanning; `null` without a baseline. */
  fraction: number | null
  /** Remaining time, once enough of the scan has run to project it. */
  etaMs: number | null
  /** The scan has passed its baseline; only the last stretch is left. */
  finishing: boolean
  bytesPerSecond: number | null
}

/** Never claim completion before the host does. */
const MAX_FRACTION = 0.99
/** Too early to project: a few large files would swing the estimate. */
const MIN_FRACTION_FOR_ETA = 0.02
const MIN_ELAPSED_FOR_ETA_MS = 3_000

function trimmed(path: string): string {
  return path.replace(/\/+$/, '') || '/'
}

export function scanBaseline(
  label: string | null,
  paths: readonly string[],
  history: Readonly<Record<string, ScanSizeRecord>>,
  volumes: readonly ScanVolume[],
): ScanBaseline | null {
  const previous = label === null ? undefined : history[label]
  if (previous && previous.bytes > 0) {
    return { bytes: previous.bytes, entries: previous.entries > 0 ? previous.entries : null, source: 'last-scan' }
  }
  // Only a scan of a whole mounted volume can borrow its used space; a folder
  // is some unknown fraction of its disk.
  const mounts = new Map(volumes.map((volume) => [trimmed(volume.path), volume]))
  let used = 0
  for (const path of new Set(paths.map(trimmed))) {
    const volume = mounts.get(path)
    if (volume) used += Math.max(0, volume.totalBytes - volume.availableBytes)
  }
  return used > 0 ? { bytes: used, entries: null, source: 'disk-used' } : null
}

export function estimateScanProgress(input: {
  bytesScanned: number
  entriesScanned: number
  elapsedMs: number
  baseline: ScanBaseline | null
}): ScanProgressEstimate {
  const { bytesScanned, entriesScanned, elapsedMs, baseline } = input
  const bytesPerSecond = elapsedMs > 0 ? (bytesScanned / elapsedMs) * 1000 : null
  if (baseline === null || baseline.bytes <= 0) {
    return { fraction: null, etaMs: null, finishing: false, bytesPerSecond }
  }
  // Bytes alone jump on large files; the item count of the last scan of the
  // same place moves more evenly, so both are averaged when known.
  const byBytes = bytesScanned / baseline.bytes
  const raw = baseline.entries ? (byBytes + entriesScanned / baseline.entries) / 2 : byBytes
  const finishing = raw >= MAX_FRACTION
  const fraction = Math.max(0, Math.min(raw, MAX_FRACTION))
  const etaMs =
    !finishing && fraction >= MIN_FRACTION_FOR_ETA && elapsedMs >= MIN_ELAPSED_FOR_ETA_MS
      ? (elapsedMs * (1 - fraction)) / fraction
      : null
  return { fraction, etaMs, finishing, bytesPerSecond }
}

/** `45 s`, `3 min`, `1 h 5 min`. */
export function formatDuration(ms: number): string {
  const seconds = Math.max(0, Math.round(ms / 1000))
  if (seconds < 60) return `${seconds} s`
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes} min`
  const hours = Math.floor(minutes / 60)
  const rest = minutes % 60
  return rest === 0 ? `${hours} h` : `${hours} h ${rest} min`
}
