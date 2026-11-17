<script lang="ts">
  /**
   * The memory split by application: one thin track of proportional segments,
   * with a legend underneath that names them. The largest groups get their own
   * segment; the tail folds into `Other (n)`. Clicking a segment or a legend
   * entry selects its group so the table can scroll to the row.
   *
   * Accessibility (§6.5): the legend is the keyboard path. Each entry is a
   * button labelled `name, size, percent` with `aria-pressed`. The track is
   * decoration for pointer users and is hidden from assistive tech.
   *
   * Props only; no transport.
   */
  import type { ProcessGroup } from '@space-lens/contract'
  import { formatBytes, formatShare } from '../../lib/memory/format.ts'
  import { layoutSegments } from '../../lib/memory/segments.ts'
  import { seriesColor } from '../../lib/palette.ts'

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

  function keyFor(groupId: string | null): string | null {
    return groupId === null ? null : `group:${groupId}`
  }

  function colorFor(rank: number, groupId: string | null): string {
    return groupId === null ? 'var(--muted-foreground)' : seriesColor(rank)
  }
</script>

<div class={`memory-segment-bar w-full space-y-2 ${className}`} bind:clientWidth={widthPx}>
  {#if segments.length === 0}
    <div class="h-2.5 w-full rounded-full bg-muted" aria-hidden="true"></div>
  {:else}
    <div class="flex h-2.5 w-full gap-px overflow-hidden rounded-full bg-muted" aria-hidden="true">
      {#each segments as segment (segment.groupId ?? 'other')}
        {@const key = keyFor(segment.groupId)}
        {@const selected = key !== null && selection.has(key)}
        <button
          type="button"
          tabindex="-1"
          class="h-full min-w-[3px] transition-opacity hover:opacity-80"
          class:opacity-40={selection.size > 0 && !selected}
          style={`width: ${segment.share * 100}%; background-color: ${colorFor(segment.rank, segment.groupId)}`}
          title={`${segment.label} — ${formatBytes(segment.bytes)} (${formatShare(segment.share)})`}
          onclick={() => {
            if (key !== null) onSelect(key)
          }}
        ></button>
      {/each}
    </div>

    <ul role="list" class="flex flex-wrap gap-x-1 gap-y-1 text-xs">
      {#each segments as segment (segment.groupId ?? 'other')}
        {@const key = keyFor(segment.groupId)}
        {@const selected = key !== null && selection.has(key)}
        <li>
          <button
            type="button"
            disabled={key === null}
            aria-label={`${segment.label}, ${formatBytes(segment.bytes)}, ${formatShare(segment.share)}`}
            aria-pressed={selected}
            class={`flex items-center gap-1.5 rounded-full border px-2.5 py-1 tabular-nums transition-colors hover:bg-muted disabled:cursor-default disabled:hover:bg-transparent ${selected ? 'border-foreground/40 bg-muted' : 'border-transparent'}`}
            onclick={() => {
              if (key !== null) onSelect(key)
            }}
          >
            <span class="size-2 shrink-0 rounded-full" style:background={colorFor(segment.rank, segment.groupId)}></span>
            <span class="max-w-40 truncate">{segment.label}</span>
            <span class="text-muted-foreground">{formatBytes(segment.bytes)}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>
