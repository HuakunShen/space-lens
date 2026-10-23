<script lang="ts">
  /**
   * The iStatistica-style segment bar (§6.2 item 2): the largest groups by
   * footprint as a share of used memory, coloured by rank on the pink → purple →
   * teal ramp from `lib/memory/segments.ts` (rank, not identity — the identity
   * lives in the label). Clicking a segment selects its group so the table can
   * scroll to the row. Thinner-than-2-px groups and the tail fold into a single
   * `Other (n)` segment, exactly as `layoutSegments` decides.
   *
   * Accessibility (§6.5): a `role="list"` of buttons, each labelled
   * `name, size, percent`.
   *
   * Props only; no transport.
   */
  import type { ProcessGroup } from '@space-lens/contract'
  import { formatBytes, formatShare } from '../../lib/memory/format.ts'
  import { layoutSegments, segmentColor } from '../../lib/memory/segments.ts'

  interface Props {
    groups: readonly ProcessGroup[]
    usedBytes: number
    selection: ReadonlySet<string>
    onSelect: (key: string) => void
    class?: string
  }

  let { groups, usedBytes, selection, onSelect, class: className = '' }: Props = $props()

  let widthPx = $state(0)
  let segments = $derived(layoutSegments(groups, usedBytes, widthPx))
  const LABEL_MIN_SHARE = 0.06

  function keyFor(groupId: string | null): string | null {
    return groupId === null ? null : `group:${groupId}`
  }
</script>

<div class={`memory-segment-bar w-full ${className}`} bind:clientWidth={widthPx}>
  {#if segments.length === 0}
    <div class="h-8 w-full rounded-md bg-muted" aria-hidden="true"></div>
  {:else}
    <ul role="list" class="flex h-8 w-full overflow-hidden rounded-md bg-muted">
      {#each segments as segment (segment.groupId ?? 'other')}
        {@const key = keyFor(segment.groupId)}
        {@const selected = key !== null && selection.has(key)}
        <li class="h-full" style={`width: ${segment.share * 100}%`}>
          <button
            type="button"
            class={`group relative flex h-full w-full items-center justify-center overflow-hidden px-0.5 text-[11px] leading-none text-white ${selected ? 'ring-2 ring-foreground ring-inset' : ''}`}
            style={`background-color: ${segmentColor(segment.rank, segments.length)}`}
            aria-label={`${segment.label}, ${formatBytes(segment.bytes)}, ${formatShare(segment.share)}`}
            title={`${segment.label} — ${formatBytes(segment.bytes)} (${formatShare(segment.share)})`}
            aria-pressed={selected}
            onclick={() => {
              if (key !== null) onSelect(key)
            }}
          >
            {#if segment.share >= LABEL_MIN_SHARE}
              <span class="max-w-full truncate drop-shadow-[0_1px_1px_rgba(0,0,0,0.6)]">
                {segment.label}
              </span>
            {/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>
