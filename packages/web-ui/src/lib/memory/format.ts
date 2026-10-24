/**
 * Number formatting for the Memory view (§6 of docs/memory-cleaner-plan.md).
 *
 * This is deliberately separate from `lib/format.ts`: that module formats disk
 * sizes as `MiB`/`GiB` for the storage view, while the memory view follows
 * Activity-Monitor-style labels (`MB`/`GB`, binary divisors) in status lines
 * and reason text. No file in this module imports the transport or the app.
 */

const BYTE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'] as const

/** Human byte size, e.g. `757 MB`, `31.2 GB`. Non-finite or non-positive input is `0 B`. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B'
  let value = bytes
  let unit = 0
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024
    unit += 1
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1
  return `${value.toFixed(digits)} ${BYTE_UNITS[unit]}`
}

/**
 * A CPU percentage without the `%` sign, e.g. `0.4` or `43`. Values are already
 * percentages (0..100+), never ratios.
 */
export function formatPercentValue(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '0'
  return value.toFixed(value >= 10 ? 0 : 1)
}

/** A 0..1 share as a whole-percent label, e.g. `6%`. */
export function formatShare(share: number): string {
  if (!Number.isFinite(share) || share <= 0) return '0%'
  return `${Math.round(share * 100)}%`
}

/**
 * A running duration, e.g. `3d 4h`, `2h 12m`, `8m 4s`, `12s`.
 *
 * Long durations drop their smaller units: a process older than a day is not
 * better understood by being told its seconds. Negative or non-finite input is
 * unknown rather than `0s`.
 */
export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '—'
  const total = Math.floor(seconds)
  const days = Math.floor(total / 86_400)
  const hours = Math.floor((total % 86_400) / 3_600)
  const minutes = Math.floor((total % 3_600) / 60)
  const rest = total % 60
  if (days > 0) return `${days}d ${hours}h`
  if (hours > 0) return `${hours}h ${minutes}m`
  if (minutes > 0) return `${minutes}m ${rest}s`
  return `${rest}s`
}

/** The local start time of a microsecond timestamp, or `null` when unknown. */
export function formatStartTime(startTimeUs: number): string | null {
  if (!Number.isFinite(startTimeUs) || startTimeUs <= 0) return null
  return new Date(startTimeUs / 1_000).toLocaleString()
}

/** How long a microsecond start timestamp has been running at `sampledAt`. */
export function runningSeconds(startTimeUs: number, sampledAt: string): number | null {
  if (!Number.isFinite(startTimeUs) || startTimeUs <= 0) return null
  const sampledAtMs = Date.parse(sampledAt)
  if (Number.isNaN(sampledAtMs)) return null
  return Math.max(0, (sampledAtMs - startTimeUs / 1_000) / 1_000)
}

/** How long an ISO start timestamp has been running at `sampledAt`. */
export function runningSecondsFromIso(startedAt: string, sampledAt: string): number | null {
  const startedAtMs = Date.parse(startedAt)
  const sampledAtMs = Date.parse(sampledAt)
  // A group whose members had no readable start time serializes as the Unix
  // epoch; that is unknown, not a 56-year-old process.
  if (!Number.isFinite(startedAtMs) || startedAtMs <= 0 || Number.isNaN(sampledAtMs)) return null
  return Math.max(0, (sampledAtMs - startedAtMs) / 1_000)
}
