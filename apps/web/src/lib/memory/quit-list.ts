/**
 * Client-side Quit list model for the Memory view (T20, §6.2 item 6).
 *
 * A plain array reducer the page owns in memory; the AI advisor (T22) stages
 * into the same list. Nothing here executes — the page turns the list into a
 * graceful terminate plan when the user picks "Review & quit…".
 */
import type { TerminatePlan } from '@space-lens/contract'

/** Where a staged quit came from; the plan and audit use the same vocabulary. */
export type TerminateOrigin = TerminatePlan['origin']

/** One staged Quit-list entry. Structurally identical to the panel's model. */
export interface QuitListEntry {
  /** Stable key: `group:<groupId>` or `proc:<pid>:<startUs>`. */
  key: string
  groupId: string
  name: string
  origin: TerminateOrigin
  /** The plain-language reason shown under the name, or `null`. */
  reason: string | null
  stagedAt: string
}

/** The Quit list holds at most 20 entries (§8). */
export const QUIT_LIST_CAP = 20

/**
 * Add one entry, de-duplicating by `key` (the newest entry wins and moves to
 * the end) and capping the list at `QUIT_LIST_CAP`, dropping the oldest. The
 * input list is never mutated.
 */
export function addToQuitList(list: readonly QuitListEntry[], e: QuitListEntry): QuitListEntry[] {
  const without = list.filter((entry) => entry.key !== e.key)
  const next = [...without, e]
  return next.length > QUIT_LIST_CAP ? next.slice(next.length - QUIT_LIST_CAP) : next
}
