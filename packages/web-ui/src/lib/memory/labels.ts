/**
 * Display labels and tones for the process enums. Kept next to the memory
 * helpers so the table, the tree, the stale list and the dialog spell a state
 * the same way.
 */
import type { ProcessState } from '@space-lens/contract'

export const PROCESS_STATE_LABELS: Record<ProcessState, string> = {
  running: 'Running',
  sleeping: 'Sleeping',
  idle: 'Idle',
  stopped: 'Stopped',
  zombie: 'Zombie',
  unknown: 'Unknown',
}

/** The dot colour beside a process name / in the state column. */
export const PROCESS_STATE_DOT: Record<ProcessState, string> = {
  running: 'bg-emerald-500',
  sleeping: 'bg-slate-400/80 dark:bg-slate-500/80',
  idle: 'bg-amber-500',
  stopped: 'bg-orange-500',
  zombie: 'bg-destructive',
  unknown: 'bg-muted-foreground/40',
}

/** The text colour when the state is spelled out. */
export const PROCESS_STATE_TEXT: Record<ProcessState, string> = {
  running: 'text-emerald-600 dark:text-emerald-400',
  sleeping: 'text-muted-foreground',
  idle: 'text-amber-600 dark:text-amber-400',
  stopped: 'text-orange-600 dark:text-orange-400',
  zombie: 'text-destructive',
  unknown: 'text-muted-foreground',
}
