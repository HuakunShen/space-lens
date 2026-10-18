<script lang="ts">
  /**
   * Header card for the Memory view (§6.2 item 1): the pressure level word with
   * its colour, the used / swap / compressed figures, and a 10-minute sparkline
   * of used + compressed memory when history is available. The level word is
   * always rendered (colour is never the only signal) and sits alone inside a
   * polite live region so a level change is announced without per-tick numbers.
   *
   * Props only; no transport.
   */
  import type { MemoryHistory, MemoryPressure, MemorySnapshot } from '@space-lens/contract'
  import { Card } from '../ui/card/index'
  import { formatBytes } from '../../lib/memory/format.ts'
  import Sparkline from './Sparkline.svelte'

  interface Props {
    snapshot: MemorySnapshot
    /** Live history; the sparkline is hidden when absent or too short. */
    history?: MemoryHistory | null
    class?: string
  }

  let { snapshot, history = null, class: className = '' }: Props = $props()

  const LEVEL_LABELS: Record<MemoryPressure['level'], string> = {
    normal: 'Normal',
    warning: 'Warning',
    critical: 'Critical',
    unknown: 'Unknown',
  }

  const LEVEL_CLASSES: Record<MemoryPressure['level'], string> = {
    normal: 'text-emerald-600 dark:text-emerald-400',
    warning: 'text-amber-600 dark:text-amber-400',
    critical: 'text-destructive',
    unknown: 'text-muted-foreground',
  }

  const SPARKLINE_WINDOW_MS = 10 * 60 * 1000

  let pressure = $derived(snapshot.pressure)
  let levelLabel = $derived(LEVEL_LABELS[pressure.level])
  let levelClass = $derived(LEVEL_CLASSES[pressure.level])
  let compressed = $derived(snapshot.breakdown.compressedBytes)

  /** The tail of the history covering the last ten minutes, oldest first. */
  let windowPoints = $derived.by(() => {
    if (!history || history.points.length === 0 || history.stepMs <= 0) return []
    const keep = Math.max(2, Math.min(history.points.length, Math.round(SPARKLINE_WINDOW_MS / history.stepMs)))
    return history.points.slice(-keep)
  })
  let usedSeries = $derived(windowPoints.map((point) => point.usedBytes))
  let compressedSeries = $derived(windowPoints.map((point) => point.compressedBytes))
  let showSparkline = $derived(windowPoints.length > 1)
</script>

<Card class={`memory-pressure-card ${className}`}>
  <div class="flex flex-col gap-2 p-4">
    <div class="flex items-start justify-between gap-4">
      <div class="min-w-0">
        <p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">Memory pressure</p>
        <p class="text-2xl font-semibold" aria-live="polite" aria-atomic="true">
          <span class={levelClass}>{levelLabel}</span>
        </p>
      </div>
      <dl class="flex flex-wrap justify-end gap-x-5 gap-y-1 text-right text-sm">
        <div>
          <dt class="text-xs text-muted-foreground">Used</dt>
          <dd class="font-medium tabular-nums">
            {formatBytes(snapshot.breakdown.usedBytes)} of {formatBytes(snapshot.breakdown.totalBytes)}
          </dd>
        </div>
        <div>
          <dt class="text-xs text-muted-foreground">Swap</dt>
          <dd class="font-medium tabular-nums">{formatBytes(snapshot.breakdown.swapUsedBytes)} used</dd>
        </div>
        <div>
          <dt class="text-xs text-muted-foreground">Compressed</dt>
          <dd class="font-medium tabular-nums">
            {compressed === null ? 'Unavailable' : formatBytes(compressed)}
          </dd>
        </div>
      </dl>
    </div>

    {#if showSparkline}
      <div class="flex items-center gap-3">
        <Sparkline
          values={usedSeries}
          compare={compressedSeries}
          height={40}
          label="Used memory, last 10 minutes, with compressed memory behind it"
          class="min-w-0 flex-1"
        />
        <span class="sr-only">
          Used memory and compressed memory over the last ten minutes.
        </span>
      </div>
    {/if}

    {#if pressure.source === 'derived'}
      <p class="text-xs text-muted-foreground">
        This platform has no kernel pressure reading, so the level is estimated from available memory.
      </p>
    {/if}
  </div>
</Card>
