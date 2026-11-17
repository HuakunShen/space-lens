<script lang="ts">
  /**
   * One preset's memory at a glance: the total, its share of used memory, and
   * one row per program. Clicking a row scopes the table, tree and explorer to
   * that program; clicking it again, or "Show all", clears the scope.
   *
   * Props only; the page owns the snapshot and the selection.
   */
  import type { PresetMemorySummary } from '../../lib/presets/index.ts'
  import { formatBytes, formatShare } from '../../lib/memory/format.ts'
  import { seriesColor } from '../../lib/palette.ts'

  interface Props {
    label: string
    description: string
    summary: PresetMemorySummary
    usedBytes: number
    /** The selected rule id, or null for the whole preset. */
    selected: string | null
    onSelect: (ruleId: string | null) => void
    class?: string
  }

  let { label, description, summary, usedBytes, selected, onSelect, class: className = '' }: Props = $props()

  const colorById = $derived(new Map(summary.rows.map((row, index) => [row.rule.id, seriesColor(index)])))
  const running = $derived(summary.rows.filter((row) => row.bytes > 0).sort((a, b) => b.bytes - a.bytes))
  const idle = $derived(summary.rows.filter((row) => row.bytes === 0).map((row) => row.rule.name))
  const largest = $derived(running[0]?.bytes ?? 0)
  const share = $derived(usedBytes > 0 ? summary.totalBytes / usedBytes : 0)
  const selectedName = $derived(summary.rows.find((row) => row.rule.id === selected)?.rule.name ?? null)
</script>

<section
  class={`rounded-lg border bg-card p-4 text-card-foreground shadow-sm ${className}`}
  aria-label={`${label} memory`}
>
  <header class="flex items-start gap-3">
    <div class="min-w-0 flex-1">
      <p class="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">{label}</p>
      <div class="mt-1 flex flex-wrap items-baseline gap-x-2">
        <strong class="text-2xl font-semibold tabular-nums">{formatBytes(summary.totalBytes)}</strong>
        <span class="text-xs text-muted-foreground tabular-nums">
          {formatShare(share)} of used memory · {summary.processCount.toLocaleString()}
          {summary.processCount === 1 ? 'process' : 'processes'}
        </span>
      </div>
      <p class="mt-1 text-xs text-muted-foreground">{description}</p>
    </div>
    {#if selected !== null}
      <button
        type="button"
        class="shrink-0 rounded-md border px-2 py-1 text-xs hover:bg-muted"
        onclick={() => onSelect(null)}>Show all</button
      >
    {/if}
  </header>

  {#if running.length === 0}
    <p class="mt-4 text-sm text-muted-foreground">None of these programs is running.</p>
  {:else}
    <ul class="mt-4 space-y-1" aria-label={`${label} by program`}>
      {#each running as row (row.rule.id)}
        {@const pressed = selected === row.rule.id}
        <li>
          <button
            type="button"
            aria-pressed={pressed}
            class={`flex w-full flex-col gap-1.5 rounded-md px-2 py-1.5 text-left transition-colors hover:bg-muted ${pressed ? 'bg-muted ring-1 ring-foreground/25' : ''}`}
            onclick={() => onSelect(pressed ? null : row.rule.id)}
          >
            <span class="flex w-full items-baseline gap-2 text-sm">
              <span class="size-2 shrink-0 rounded-full" style:background={colorById.get(row.rule.id)}></span>
              <span class="min-w-0 flex-1 truncate">{row.rule.name}</span>
              <span class="shrink-0 text-xs text-muted-foreground tabular-nums">
                {row.processCount} {row.processCount === 1 ? 'proc' : 'procs'}
              </span>
              <span class="w-20 shrink-0 text-right font-medium tabular-nums">{formatBytes(row.bytes)}</span>
            </span>
            <span class="block h-1 w-full overflow-hidden rounded-full bg-muted">
              <span
                class="block h-full rounded-full"
                style:width={`${largest > 0 ? (row.bytes / largest) * 100 : 0}%`}
                style:background={colorById.get(row.rule.id)}
              ></span>
            </span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <footer class="mt-3 space-y-0.5 border-t pt-2 text-[11px] text-muted-foreground">
    {#if selectedName !== null}
      <p>Showing {selectedName} in the views below.</p>
    {:else if running.length > 1}
      <p>Click a program to see its processes below. Nested programs count under their parent.</p>
    {/if}
    {#if idle.length > 0}
      <p>Not running: {idle.join(', ')}</p>
    {/if}
  </footer>
</section>
