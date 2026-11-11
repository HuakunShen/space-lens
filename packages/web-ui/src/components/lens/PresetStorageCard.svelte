<script lang="ts">
  /**
   * A preset's disk use from the App storage audit: the total and one row per
   * program. Clicking a row narrows the app list below to that program; clicking
   * it again, or "Show all", widens it again.
   *
   * Props only; the audit report and the filter are owned by the caller.
   */
  import type { PresetStorageSummary } from '../../lib/presets/index.ts'
  import { formatBytes } from '../../lib/format'
  import { seriesColor } from '../../lib/palette.ts'

  interface Props {
    label: string
    description: string
    summary: PresetStorageSummary
    /** Whether the audit has finished, so zeros mean "none found" and not "loading". */
    ready: boolean
    selected: string | null
    onSelect: (ruleId: string | null) => void
  }

  let { label, description, summary, ready, selected, onSelect }: Props = $props()

  const colorById = $derived(new Map(summary.rows.map((row, index) => [row.rule.id, seriesColor(index)])))
  const found = $derived(summary.rows.filter((row) => row.bytes > 0).sort((a, b) => b.bytes - a.bytes))
  const largest = $derived(found[0]?.bytes ?? 0)
  const selectedName = $derived(summary.rows.find((row) => row.rule.id === selected)?.rule.name ?? null)
</script>

<section class="rounded-lg border bg-card p-4 text-card-foreground shadow-sm" aria-label={`${label} disk use`}>
  <header class="flex items-start gap-3">
    <div class="min-w-0 flex-1">
      <p class="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">{label}</p>
      <div class="mt-1 flex flex-wrap items-baseline gap-x-2">
        <strong class="text-2xl font-semibold tabular-nums">{formatBytes(summary.totalBytes)}</strong>
        <span class="text-xs text-muted-foreground">on disk</span>
      </div>
      <p class="mt-1 text-xs text-muted-foreground">{description}</p>
    </div>
    {#if selected !== null}
      <button type="button" class="shrink-0 rounded-md border px-2 py-1 text-xs hover:bg-muted" onclick={() => onSelect(null)}>
        Show all
      </button>
    {/if}
  </header>

  {#if found.length === 0}
    <p class="mt-4 text-sm text-muted-foreground">
      {ready ? 'No files found for these programs.' : 'Reading the audit…'}
    </p>
  {:else}
    <ul class="mt-4 space-y-1" aria-label={`${label} by program`}>
      {#each found as row (row.rule.id)}
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

  <footer class="mt-3 border-t pt-2 text-[11px] text-muted-foreground">
    {#if selectedName !== null}
      <p>Showing {selectedName} only.</p>
    {:else}
      <p>Click a program to list only its apps below.</p>
    {/if}
  </footer>
</section>
