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
