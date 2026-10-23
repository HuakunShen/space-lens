<script lang="ts">
  /**
   * Header card for the Memory view (§6.2 item 1): the pressure level with its
   * colour, the used / total figure, a share bar, a 10-minute sparkline of
   * used + compressed memory, and the swap / compressed / free figures.
   *
   * The level word is always rendered (colour is never the only signal) and
   * sits alone inside a polite live region so a level change is announced
   * without per-tick numbers. The card stretches to the height of the
   * distribution card beside it, so the header row reads as one band.
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

  const LEVEL_DOTS: Record<MemoryPressure['level'], string> = {
    normal: 'bg-emerald-500',
    warning: 'bg-amber-500',
    critical: 'bg-destructive',
    unknown: 'bg-muted-foreground',
  }

  const LEVEL_BARS: Record<MemoryPressure['level'], string> = {
    normal: 'bg-emerald-500/80',
    warning: 'bg-amber-500/80',
    critical: 'bg-destructive/80',
    unknown: 'bg-muted-foreground/50',
  }

  const SPARKLINE_WINDOW_MS = 10 * 60 * 1000

  let pressure = $derived(snapshot.pressure)
  let levelLabel = $derived(LEVEL_LABELS[pressure.level])
  let levelClass = $derived(LEVEL_CLASSES[pressure.level])
  let compressed = $derived(snapshot.breakdown.compressedBytes)

  let total = $derived(snapshot.breakdown.totalBytes > 0 ? snapshot.breakdown.totalBytes : 1)
  let usedPercent = $derived(
    Math.max(0, Math.min(100, (snapshot.breakdown.usedBytes / total) * 100)),
  )
  let usedPercentLabel = $derived(
    usedPercent > 0 && usedPercent < 0.1 ? '<0.1' : usedPercent.toFixed(usedPercent < 10 ? 1 : 0),
  )

  /** The tail of the history covering the last ten minutes, oldest first. */
  let windowPoints = $derived.by(() => {
    if (!history || history.points.length === 0 || history.stepMs <= 0) return []
    const keep = Math.max(2, Math.min(history.points.length, Math.round(SPARKLINE_WINDOW_MS / history.stepMs)))
    return history.points.slice(-keep)
  })
  let usedSeries = $derived(windowPoints.map((point) => point.usedBytes))
  let compressedSeries = $derived(windowPoints.map((point) => point.compressedBytes))
  let showSparkline = $derived(windowPoints.length >= 4)
</script>

<Card class={`memory-pressure-card h-full gap-0 py-0 ${className}`}>
  <div class="flex h-full flex-col gap-3 p-4">
    <div class="flex items-start justify-between gap-4">
      <div class="flex min-w-0 items-center gap-2.5">
        <span class={`mt-0.5 inline-flex size-2.5 shrink-0 rounded-full ${LEVEL_DOTS[pressure.level]}`} aria-hidden="true"></span>
        <div class="min-w-0">
          <p class="text-[10px] font-medium tracking-[0.14em] text-muted-foreground uppercase">
            Memory pressure
          </p>
          <p class="text-lg leading-tight font-semibold" aria-live="polite" aria-atomic="true">
            <span class={levelClass}>{levelLabel}</span>
          </p>
        </div>
      </div>
      <div class="text-right">
        <p class="text-2xl leading-none font-semibold tracking-tight tabular-nums">
          {formatBytes(snapshot.breakdown.usedBytes)}
        </p>
        <p class="mt-1 text-[11px] text-muted-foreground tabular-nums">
          of {formatBytes(snapshot.breakdown.totalBytes)} · {usedPercentLabel}%
        </p>
      </div>
    </div>

    <div
      class="h-1.5 w-full overflow-hidden rounded-full bg-muted"
      role="img"
      aria-label={`${formatBytes(snapshot.breakdown.usedBytes)} of ${formatBytes(snapshot.breakdown.totalBytes)} used (${usedPercentLabel}%)`}
    >
      <div
        class={`h-full rounded-full transition-[width] duration-500 motion-reduce:transition-none ${LEVEL_BARS[pressure.level]}`}
        style={`width: ${usedPercent}%`}
      ></div>
    </div>

    {#if showSparkline}
      <div class="flex min-h-10 flex-1 items-end">
        <Sparkline
          values={usedSeries}
          compare={compressedSeries}
          height={52}
          fill
          label="Used memory, last 10 minutes, with compressed memory behind it"
          class="w-full text-sky-500 dark:text-sky-400"
        />
      </div>
    {:else}
      <div class="flex-1"></div>
    {/if}

    <dl class="grid grid-cols-3 gap-2 border-t border-border/60 pt-2.5 text-[11px]">
      <div>
        <dt class="text-muted-foreground">Swap</dt>
        <dd class="font-medium tabular-nums">{formatBytes(snapshot.breakdown.swapUsedBytes)}</dd>
      </div>
      <div>
        <dt class="text-muted-foreground">Compressed</dt>
        <dd class="font-medium tabular-nums">
          {compressed === null ? 'Unavailable' : formatBytes(compressed)}
        </dd>
      </div>
      <div class="text-right">
        <dt class="text-muted-foreground">Free</dt>
        <dd class="font-medium tabular-nums">{formatBytes(snapshot.breakdown.freeBytes)}</dd>
      </div>
    </dl>

    {#if pressure.source === 'derived'}
      <p class="text-[10px] leading-snug text-muted-foreground">
        Estimated from available memory — this platform has no kernel pressure reading.
      </p>
    {/if}
  </div>
</Card>
