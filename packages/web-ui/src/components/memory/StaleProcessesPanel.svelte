<script lang="ts">
  /**
   * The Stale section: advisory leftovers, never an automatic kill list.
   *
   * Rows are processes the heuristic in `lib/memory/stale.ts` finds
   * suspicious — orphaned runtimes, zombies/stopped processes, processes idle
   * for hours. A listening port is shown as evidence, not as guilt. The user
   * opens the detail dialog to decide; this panel only offers a direct Quit
   * shortcut for the obvious cases.
   */
  import type { ProcessRow } from '@space-lens/contract'
  import { CheckCircle2, ClockAlert } from '@lucide/svelte'
  import { Badge } from '../ui/badge/index'
  import { Button } from '../ui/button/index'
  import { formatDuration, formatPercentValue } from '../../lib/memory/format.ts'
  import {
    STALE_REASON_LABELS,
    type StaleProcess,
    type StaleReason,
  } from '../../lib/memory/stale.ts'
  import ProcessStateDot from './ProcessStateDot.svelte'

  interface Props {
    processes: readonly StaleProcess[]
    terminateAvailable?: boolean
    onOpenProcess?: (process: ProcessRow) => void
    onQuit?: (process: ProcessRow) => void
    class?: string
  }

  let {
    processes,
    terminateAvailable = false,
    onOpenProcess,
    onQuit,
    class: className = '',
  }: Props = $props()

  const REASON_CLASS: Record<StaleReason, string> = {
    zombie: 'border-destructive/30 bg-destructive/10 text-destructive',
    stopped: 'border-orange-500/30 bg-orange-500/10 text-orange-700 dark:text-orange-300',
    runtimeLeftover: 'border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300',
    orphaned: 'border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300',
    longIdle: 'border-border bg-muted/40 text-muted-foreground',
    port: 'border-sky-500/30 bg-sky-500/10 text-sky-700 dark:text-sky-300',
  }

  function quit(process: ProcessRow): void {
    if (process.protection !== null || !terminateAvailable) return
    onQuit?.(process)
  }
</script>

<section class={`flex flex-col gap-3 ${className}`} aria-label="Possibly stale processes">
  <div class="rounded-md border bg-card/40 px-4 py-3">
    <div class="flex flex-wrap items-center gap-2">
      <ClockAlert class="size-4 text-amber-500" />
      <h2 class="text-sm font-semibold">Possibly stale processes</h2>
      {#if processes.length > 0}<Badge variant="secondary">{processes.length}</Badge>{/if}
    </div>
    <p class="mt-1 text-xs leading-relaxed text-muted-foreground">
      Advisory only — Space Lens never quits these automatically. A process is listed when its
      parent exited and a runtime was left behind, it is a zombie or stopped, or a runtime has been
      idle for hours. A listening port is evidence, not a verdict.
    </p>
  </div>

  {#if processes.length === 0}
    <div class="flex flex-col items-center gap-2 rounded-md border bg-card/30 px-4 py-10 text-center">
      <CheckCircle2 class="size-6 text-emerald-500" />
      <p class="text-sm font-medium">No stale processes detected</p>
      <p class="max-w-md text-xs text-muted-foreground">
        Nothing currently matches the orphaned-runtime, zombie, stopped or long-idle rules.
      </p>
    </div>
  {:else}
    <ul role="list" class="overflow-hidden rounded-md border">
      {#each processes as entry (entry.process.pid)}
        {@const process = entry.process}
        <li class="border-b last:border-b-0">
          <div class="flex flex-wrap items-start gap-3 px-3 py-2.5 hover:bg-muted/30">
            <button
              type="button"
              class="flex min-w-0 flex-1 flex-col gap-1 text-left"
              title={process.command ?? process.name}
              onclick={() => onOpenProcess?.(process)}
            >
              <span class="flex min-w-0 items-center gap-2">
                <ProcessStateDot state={process.state} />
                <span class="truncate text-sm font-medium">{process.name}</span>
                <span class="shrink-0 text-xs text-muted-foreground">PID {process.pid}</span>
                {#if entry.group !== null}<span class="truncate text-xs text-muted-foreground">· {entry.group.name}</span>{/if}
              </span>
              <span class="flex flex-wrap items-center gap-1">
                {#each entry.reasons as reason (reason)}
                  <Badge variant="outline" class={REASON_CLASS[reason]}>{STALE_REASON_LABELS[reason]}</Badge>
                {/each}
                {#each process.listeningPorts as port (port)}
                  <Badge variant="outline" class="border-sky-500/30 bg-sky-500/10 font-mono text-sky-700 dark:text-sky-300">
                    :{port}
                  </Badge>
                {/each}
              </span>
              {#if process.command !== null}
                <span class="truncate font-mono text-[11px] text-muted-foreground">{process.command}</span>
              {/if}
            </button>
            <div class="flex shrink-0 flex-col items-end gap-1">
              <span class="text-xs tabular-nums text-muted-foreground">
                {formatDuration(entry.ageSeconds)} · {process.cpuPercent === null ? '—' : `${formatPercentValue(process.cpuPercent)}%`}
              </span>
              <div class="flex items-center gap-1">
                <Button variant="ghost" size="xs" onclick={() => onOpenProcess?.(process)}>Inspect</Button>
                {#if terminateAvailable && process.protection === null}
                  <Button variant="outline" size="xs" onclick={() => quit(process)}>Quit</Button>
                {/if}
              </div>
            </div>
          </div>
        </li>
      {/each}
    </ul>
    <p class="text-xs text-muted-foreground">
      A listed process may be intentional. Open it to see the full command, start time and children
      before quitting.
    </p>
  {/if}
</section>
