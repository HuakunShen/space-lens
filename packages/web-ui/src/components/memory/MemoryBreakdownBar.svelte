<script lang="ts">
  /**
   * System memory breakdown bar (§6.2 item 1): App / Wired / Compressed / Cached
   * files / Free as one stacked bar with a legend. When the platform cannot
   * report the optional fields (`appBytes` … `cachedFilesBytes` are `null`) it
   * says so and falls back to a plain Used / Free bar.
   *
   * Props only; no transport.
   */
  import type { MemoryBreakdown } from '@space-lens/contract'
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

  function widthPercent(bytes: number): number {
    return Math.max(0, Math.min(100, (bytes / total) * 100))
  }
</script>

<div class={`memory-breakdown ${className}`}>
  <div class="flex h-3 w-full overflow-hidden rounded-full bg-muted" role="img" aria-label={ariaLabel}>
    {#each segments as segment (segment.key)}
      <div
        class="h-full transition-[width] duration-300 motion-reduce:transition-none"
        style={`width: ${widthPercent(segment.bytes)}%; background-color: ${segment.color}`}
        title={`${segment.label} — ${formatBytes(segment.bytes)}`}
      ></div>
    {/each}
  </div>

  {#if detailed === null}
    <p class="mt-2 text-xs text-muted-foreground">Breakdown unavailable on this platform.</p>
  {/if}

  <ul role="list" class="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
    {#each segments as segment (segment.key)}
      <li class="flex items-center gap-1.5">
        <span
          class="inline-block size-2.5 rounded-[3px]"
          style={`background-color: ${segment.color}`}
          aria-hidden="true"
        ></span>
        <span>{segment.label}</span>
        <span class="font-medium tabular-nums text-foreground">{formatBytes(segment.bytes)}</span>
      </li>
    {/each}
  </ul>
</div>
