<script lang="ts">
  import type { AppAuditEntry } from '@space-lens/contract'
  import { formatBytes } from '../../lib/format'
  import { initialsFor, purgeableTotal } from '../../lib/app-audit'
  import { ensureIcon, readIcon, subscribeIcons, whenVisible } from '../../lib/app-icon-cache'

  interface Props {
    entry: AppAuditEntry
    active: boolean
    onOpen: (id: string) => void
    onIcon?: (appPath: string) => Promise<string | null>
    /** Stale audit generation (badge + rescan affordance). */
    stale?: boolean
    /** A scan is starting for this app (button feedback). */
    scanning?: boolean
    scanDisabled?: boolean
    scanTitle?: string
  }
  let { entry, active, onOpen, onIcon, stale = false, scanning = false, scanDisabled = false, scanTitle }: Props = $props()

  // The icon cache is shared across cards: a failed logo stays failed and
  // never re-enqueues, and at most four fetches run at once. Icons are only
  // requested when the card scrolls near the viewport — a grid of hundreds
  // of apps must not parse every icns up front.
  let iconTick = $state(0)
  let cardEl = $state<HTMLButtonElement>()
  $effect(() => subscribeIcons(() => (iconTick += 1)))
  $effect(() => {
    const appPath = entry.appPath
    const element = cardEl
    if (!onIcon || !appPath || entry.kind === 'cli' || !element) return
    return whenVisible(element, () => ensureIcon(appPath, onIcon))
  })
  let icon = $derived.by(() => {
    void iconTick
    const appPath = entry.appPath
    if (!appPath || entry.kind === 'cli') return { status: 'failed' as const, url: null }
    return readIcon(appPath) ?? { status: 'loading' as const, url: null }
  })

  let reclaimable = $derived(purgeableTotal(entry.locations))
  let locationLabel = $derived(
    `${entry.locations.length} location${entry.locations.length === 1 ? '' : 's'}`,
  )
</script>

<button
  type="button"
  id={`app-card-${entry.id}`}
  bind:this={cardEl}
  class="flex min-w-0 flex-col gap-2 rounded-lg border bg-card p-3 text-left hover:ring-1 hover:ring-ring"
  class:ring-1={active}
  class:ring-ring={active}
  aria-pressed={active}
  aria-label={scanTitle ?? `Scan ${entry.name}`}
  title={scanTitle ?? `Scan ${entry.name}`}
  disabled={scanDisabled || scanning}
  onclick={() => onOpen(entry.id)}
>
  <div class="flex w-full items-start gap-2">
    {#if icon.status === 'ready' && icon.url}
      <img
        src={icon.url}
        alt=""
        aria-hidden="true"
        loading="lazy"
        decoding="async"
        class="size-8 shrink-0 rounded-lg object-contain"
      />
    {:else if icon.status === 'failed'}
      <div
        class="flex size-8 shrink-0 items-center justify-center rounded-lg bg-muted text-xs font-semibold text-muted-foreground"
        aria-hidden="true">{initialsFor(entry.name)}</div
      >
    {:else}
      <div class="size-8 shrink-0 animate-pulse rounded-lg bg-muted" aria-hidden="true"></div>
    {/if}
    <strong class="min-w-0 flex-1 truncate text-sm font-semibold" title={entry.name}>{entry.name}</strong>
    {#if stale}<span
        class="shrink-0 rounded-full border border-amber-500/50 px-1.5 py-px text-[10px] font-medium text-amber-600 dark:text-amber-400"
        title="Audit refreshed or files changed after cleanup — rescan for current sizes"
        role="status">Stale</span
      >{/if}
    <span class="shrink-0 text-sm font-semibold tabular-nums">{formatBytes(entry.totalSize)}</span>
  </div>
  <div class="flex w-full items-center gap-1.5 truncate text-[11px] text-muted-foreground">
    <span class="shrink-0 rounded-full border px-2 py-0.5 text-[10px] font-medium"
      >{entry.kind === 'app' ? 'App' : 'CLI'}</span
    >
    {#if entry.source === 'catalog'}<span
        class="shrink-0 rounded-full border px-2 py-0.5 text-[10px] font-medium"
        title="Curated entry with human-reviewed location notes">Known</span
      >{/if}
    <span class="min-w-0 truncate">· {locationLabel}</span>
  </div>
  {#if reclaimable > 0}
    <p class="flex w-full items-center gap-1.5 truncate text-xs text-muted-foreground">
      <span
        class="size-1.5 shrink-0 rounded-full bg-primary"
        title="Marked purgeable by classification"
      ></span>
      <span class="truncate">{formatBytes(reclaimable)} reclaimable</span>
    </p>
  {/if}
  <span class="w-full truncate text-[11px] text-muted-foreground">{scanning ? 'Starting scan…' : 'Select to scan →'}</span>
</button>
