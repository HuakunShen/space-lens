<script lang="ts">
  /**
   * System memory breakdown (§6.2 item 1): App / Wired / Compressed / Cached
   * files / Free as one stacked bar with a legend that also carries each
   * segment's share. When the platform cannot report the optional fields
   * (`appBytes` … `cachedFilesBytes` are `null`) it says so and falls back to a
   * plain Used / Free bar.
   *
   * Props only; no transport.
   */
  import type { MemoryBreakdown } from '@space-lens/contract'
  import { Card } from '../ui/card/index'
  import { formatBytes } from '../../lib/memory/format.ts'

  interface Props {
    breakdown: MemoryBreakdown
    class?: string
  }

  interface Segment {
    key: string
    label: string
    bytes: number
    color: string
  }

  let { breakdown, class: className = '' }: Props = $props()

  const COLORS = {
    app: '#5b8def',
    wired: '#f0a24f',
    compressed: '#9b5cff',
    cachedFiles: '#19d3c5',
    free: '#3d4451',
  } as const

  let total = $derived(breakdown.totalBytes > 0 ? breakdown.totalBytes : 1)

  let detailed = $derived.by((): Segment[] | null => {
    const { appBytes, wiredBytes, compressedBytes, cachedFilesBytes } = breakdown
    if (appBytes === null || wiredBytes === null || compressedBytes === null || cachedFilesBytes === null) {
      return null
    }
    return [
      { key: 'app', label: 'App', bytes: appBytes, color: COLORS.app },
      { key: 'wired', label: 'Wired', bytes: wiredBytes, color: COLORS.wired },
      { key: 'compressed', label: 'Compressed', bytes: compressedBytes, color: COLORS.compressed },
      { key: 'cachedFiles', label: 'Cached files', bytes: cachedFilesBytes, color: COLORS.cachedFiles },
      { key: 'free', label: 'Free', bytes: breakdown.freeBytes, color: COLORS.free },
    ]
  })

  let fallback = $derived<Segment[]>([
    { key: 'used', label: 'Used', bytes: breakdown.usedBytes, color: COLORS.app },
    { key: 'free', label: 'Free', bytes: breakdown.freeBytes, color: COLORS.free },
  ])

  let segments = $derived(detailed ?? fallback)

  let ariaLabel = $derived(
    `${detailed ? 'Memory breakdown' : 'Memory used and free'}: ${segments
      .map((segment) => `${segment.label} ${formatBytes(segment.bytes)}`)
      .join(', ')}`,
  )

  let usedPercent = $derived(
    Math.max(0, Math.min(100, (breakdown.usedBytes / total) * 100)),
  )

  function widthPercent(bytes: number): number {
    return Math.max(0, Math.min(100, (bytes / total) * 100))
  }

  function shareLabel(bytes: number): string {
    const percent = widthPercent(bytes)
    return percent > 0 && percent < 0.1 ? '<0.1%' : `${percent.toFixed(percent < 10 ? 1 : 0)}%`
  }
</script>

<Card class={`memory-breakdown h-full gap-0 py-0 ${className}`}>
  <div class="flex h-full flex-col justify-between gap-3 p-4">
    <div class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
      <p class="text-[10px] font-medium tracking-[0.14em] text-muted-foreground uppercase">
        Memory distribution
      </p>
      <p class="text-[11px] text-muted-foreground tabular-nums">
        {formatBytes(breakdown.usedBytes)} used · {usedPercent.toFixed(0)}%
      </p>
    </div>

    <div
      class="flex h-4 w-full overflow-hidden rounded-full bg-muted"
      role="img"
      aria-label={ariaLabel}
    >
      {#each segments as segment (segment.key)}
        <div
          class="h-full transition-[width] duration-300 motion-reduce:transition-none"
          style={`width: ${widthPercent(segment.bytes)}%; background-color: ${segment.color}`}
          title={`${segment.label} — ${formatBytes(segment.bytes)} (${shareLabel(segment.bytes)})`}
        ></div>
      {/each}
    </div>

    <ul role="list" class="grid grid-cols-1 gap-x-6 gap-y-1.5 text-xs sm:grid-cols-2 2xl:grid-cols-3">
      {#each segments as segment (segment.key)}
        <li class="flex items-center gap-2">
          <span
            class="inline-block size-2 shrink-0 rounded-[3px]"
            style={`background-color: ${segment.color}`}
            aria-hidden="true"
          ></span>
          <span class="min-w-0 flex-1 truncate text-muted-foreground">{segment.label}</span>
          <span class="font-medium tabular-nums">{formatBytes(segment.bytes)}</span>
          <span class="w-10 text-right text-[11px] text-muted-foreground tabular-nums">
            {shareLabel(segment.bytes)}
          </span>
        </li>
      {/each}
    </ul>

    {#if detailed === null}
      <p class="text-[11px] text-muted-foreground">Breakdown unavailable on this platform.</p>
    {/if}
  </div>
</Card>
