/**
 * Reason and pressure wording for the Memory view (§7 of the plan).
 * Everything is rendered from contract codes, so the UI never branches on
 * message text. The copy is user-facing and pinned by tests: `BANNED_WORDS`
 * lists the marketing words the memory UI must never use.
 */
import type { CandidateReason, MemoryPressure } from '@space-lens/contract'
import { formatBytes, formatPercentValue } from './format.ts'

/** The pressure level as carried by the contract, exported for UI callers. */
export type PressureLevel = MemoryPressure['level']

export const BANNED_WORDS = ['clean', 'boost', 'optimiz', 'purge', 'speed up'] as const

/** One candidate reason as a sentence (§7). */
export function reasonText(r: CandidateReason): string {
  switch (r.code) {
    case 'large':
      return `Using ${formatBytes(r.bytes ?? 0)}`
    case 'idle':
      return `No activity for ${r.minutes ?? 0} min while you watched (${formatPercentValue(r.cpuPercent ?? 0)}% CPU)`
    case 'background':
      return 'Runs in the background without a Dock icon'
    case 'growing':
      return `Grew from ${formatBytes(r.fromBytes ?? 0)} to ${formatBytes(r.bytes ?? 0)} in ${r.minutes ?? 0} min`
    case 'manyProcesses':
      return `${r.count ?? 0} processes`
  }
}

/** The Free-up dialog headline for a pressure level, with the release estimate (§7). */
export function pressureHeadline(level: PressureLevel, releaseBytes: number): string {
  switch (level) {
    case 'normal':
      return 'Memory pressure is normal. macOS keeps spare memory busy as cache, which makes your Mac faster — there is nothing that needs freeing. If something feels slow, quitting an idle app below is the only change that really helps.'
    case 'warning':
      return `Memory is tight: macOS is compressing memory and may use swap. Quitting the apps below would give back about ${formatBytes(releaseBytes)}.`
    case 'critical':
      return `Memory is very tight and apps may slow down or be paused. Quitting the apps below would give back about ${formatBytes(releaseBytes)}.`
    case 'unknown':
      return 'Memory pressure cannot be read on this platform right now. Quitting an idle app below is still the only change that really helps.'
  }
}
