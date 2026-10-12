<script lang="ts">
  import { Copy, LoaderCircle, RefreshCw, Search } from '@lucide/svelte'
  import type { AppAuditEntry, AppAuditReport } from '@space-lens/contract'
  import { formatBytes } from '../../lib/format'
  import {
    classificationBadge,
    classificationLabel,
    initialsFor,
    largestLocation,
    matchesQuery,
    purgeableTotal,
  } from '../../lib/app-audit'
  import LensSelect from './LensSelect.svelte'
  import { Button } from '../ui/button/index'
  import { Input } from '../ui/input/index'

  interface Props {
    report: AppAuditReport | null
    loading: boolean
    error: string | null
    minSize: number
    disabled?: boolean
    onRefresh: () => void
    onMinSize: (value: number) => void
    onCopy: (path: string) => void
    onIcon?: (appPath: string) => Promise<string | null>
  }
  let {
    report,
    loading,
    error,
    minSize,
    disabled = false,
    onRefresh,
    onMinSize,
    onCopy,
    onIcon,
  }: Props = $props()

  const sizeOptions = [
    { value: '0', label: 'All sizes' },
    { value: String(10 * 1024 ** 2), label: '≥ 10 MB' },
    { value: String(100 * 1024 ** 2), label: '≥ 100 MB' },
    { value: String(1024 ** 3), label: '≥ 1 GB' },
  ]
  let query = $state('')
  let viewMode = $state<'cards' | 'list'>('cards')
  let selectedId = $state<string | null>(null)

  let appCount = $derived(report?.apps.length ?? 0)
  let grandTotal = $derived(report?.apps.reduce((total, app) => total + app.totalSize, 0) ?? 0)
  let reclaimable = $derived(report?.apps.flatMap((app) => app.locations) ?? [])
  let reclaimableTotal = $derived(purgeableTotal(reclaimable))
  let filtered = $derived((report?.apps ?? []).filter((app) => matchesQuery(app, query)))
  let selected = $derived(report?.apps.find((app) => app.id === selectedId) ?? null)

  /** One muted summary line per card: the largest location, or the count. */
  function cardSummary(app: AppAuditEntry): string {
    const top = largestLocation(app)
    if (top) return `${top.label ?? 'Largest location'} · ${formatBytes(top.size)}`
    return `${app.locations.length} locations`
  }

  // Lazy app icons: a per-path promise cache plus a tiny queue so at most
  // four icon fetches are in flight. Entries without an app path, CLI
  // entries, and null results all fall back to the letter avatar.
  const ICON_CONCURRENCY = 4
  const iconCache = new Map<string, Promise<string | null>>()
  const queuedPaths = new Set<string>()
  const iconQueue: string[] = []
  let iconInFlight = 0
  let iconState = $state<Record<string, { status: 'loading' | 'ready' | 'failed'; url: string | null }>>({})

  function pumpIconQueue(): void {
    if (!onIcon) return
    const fetcher = onIcon
    while (iconInFlight < ICON_CONCURRENCY && iconQueue.length > 0) {
      const path = iconQueue.shift() as string
      if (iconCache.has(path)) continue
      iconInFlight += 1
      const task = Promise.resolve()
        .then(() => fetcher(path))
        .then(
          (url) => {
            iconState = { ...iconState, [path]: url ? { status: 'ready', url } : { status: 'failed', url: null } }
            return url
          },
          () => {
            iconState = { ...iconState, [path]: { status: 'failed', url: null } }
            return null
          },
        )
        .finally(() => {
          iconInFlight -= 1
          pumpIconQueue()
        })
      iconCache.set(path, task)
    }
  }

  function ensureIcon(path: string | null, kind: AppAuditEntry['kind']): void {
    if (!report || !onIcon || !path || kind === 'cli') return
    if (iconCache.has(path) || queuedPaths.has(path)) return
    queuedPaths.add(path)
    iconQueue.push(path)
    iconState = { ...iconState, [path]: { status: 'loading', url: null } }
    pumpIconQueue()
  }

  $effect(() => {
    if (!report || viewMode !== 'cards' || !onIcon) return
    for (const app of filtered) ensureIcon(app.appPath, app.kind)
  })
</script>

{#snippet appIcon(app: AppAuditEntry)}
  {@const state = app.appPath ? iconState[app.appPath] : undefined}
  {#if state?.status === 'ready' && state.url}
    <img src={state.url} alt="" class="size-12 shrink-0 rounded-lg object-contain" />
  {:else if state?.status === 'failed' || app.kind === 'cli' || !app.appPath}
    <div
      class="flex size-12 shrink-0 items-center justify-center rounded-lg bg-muted font-semibold text-muted-foreground"
      aria-hidden="true">{initialsFor(app.name)}</div
    >
  {:else}
    <div class="size-12 shrink-0 animate-pulse rounded-lg bg-muted" aria-hidden="true"></div>
  {/if}
{/snippet}

{#snippet locationList(app: AppAuditEntry)}
  <ul class="mt-3 space-y-2">
    {#each app.locations as location (location.path)}
      <li class="flex items-start gap-2 text-xs">
        {#if location.purgeable}<span
            class="mt-1.5 size-1.5 shrink-0 rounded-full bg-primary"
            title="Marked purgeable by classification"
          ></span>{/if}
        <div class="min-w-0 flex-1">
          <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span class="font-semibold tabular-nums">{formatBytes(location.size)}</span>
            <span
              class={`rounded-full border px-2 py-0.5 text-[10px] font-medium text-muted-foreground ${classificationBadge(location.classification)}`}
              >{classificationLabel(location.classification)}</span
            >
            {#if location.label}<strong>{location.label}</strong>{/if}
          </div>
          <button
            type="button"
            class="mt-0.5 block w-full truncate text-left font-mono text-muted-foreground hover:text-foreground"
            title={`${location.path} — copy path`}
            onclick={() => onCopy(location.path)}>{location.path}</button
          >
          {#if location.comment}<details class="mt-1 text-muted-foreground">
              <summary class="cursor-pointer text-[11px]">Why / how to clean</summary>
              <p class="mt-1 text-[11px] leading-relaxed">{location.comment}</p>
            </details>{/if}
        </div>
      </li>
    {/each}
  </ul>
{/snippet}

<section class="discovery-view" aria-label="App storage" aria-busy={loading}>
  <div class="discovery-heading">
    <div>
      <span class="section-eyebrow">DISCOVER & REVIEW</span>
      <h1>App storage</h1>
      <p>What installed apps and CLI tools keep on this disk</p>
    </div>
    {#if report}<div class="discovery-total">
        <strong>{formatBytes(grandTotal)}</strong><span
          >{appCount.toLocaleString()}
          app{appCount === 1 ? '' : 's'} audited</span
        >
      </div>{/if}
  </div>
  <div class="discovery-filters">
    <label class="relative flex min-w-0 flex-1 items-center gap-2"
      ><Search size={15} /><Input
        type="search"
        bind:value={query}
        placeholder="Search apps by name"
        aria-label="Search apps by name"
      /></label
    >
    <div class="setting-options w-36" role="group" aria-label="App storage view">
      <button
        type="button"
        data-selected={viewMode === 'cards'}
        class:selected={viewMode === 'cards'}
        aria-pressed={viewMode === 'cards'}
        onclick={() => (viewMode = 'cards')}>Cards</button
      >
      <button
        type="button"
        data-selected={viewMode === 'list'}
        class:selected={viewMode === 'list'}
        aria-pressed={viewMode === 'list'}
        onclick={() => (viewMode = 'list')}>List</button
      >
    </div>
    <div class="filter-select">
      <span>Size</span><LensSelect
        value={String(minSize)}
        options={sizeOptions}
        label="Minimum size"
        class="min-w-28"
        disabled={disabled || loading}
        onChange={(value) => onMinSize(Number(value))}
      />
    </div>
    <Button variant="outline" size="sm" disabled={disabled || loading} onclick={onRefresh}
      ><RefreshCw size={14} /> Refresh</Button
    >
  </div>
  {#if error}<div class="inline-error" role="alert">
      <p>{error}</p>
      <Button variant="outline" size="sm" onclick={onRefresh}>Try again</Button>
    </div>{/if}
  <div class="discovery-rows">
    {#if report}
      {#if viewMode === 'cards' && selected}
        <div class="mb-3 rounded-lg border bg-card p-4" role="region" aria-label={`Details for ${selected.name}`}>
          <div class="mb-3">
            <Button variant="outline" size="sm" onclick={() => (selectedId = null)} aria-label="Back to app grid"
              >← Back</Button
            >
          </div>
          <div class="flex items-start justify-between gap-3">
            <div class="flex min-w-0 items-start gap-3">
              {@render appIcon(selected)}
              <div class="min-w-0">
                <div class="flex flex-wrap items-center gap-2">
                  <strong class="text-sm">{selected.name}</strong>
                  <span
                    class="rounded-full border px-2 py-0.5 text-[10px] font-medium text-muted-foreground"
                    >{selected.kind === 'app' ? 'App' : 'CLI'}</span
                  >
                  {#if selected.source === 'catalog'}<span
                      class="rounded-full border px-2 py-0.5 text-[10px] font-medium text-muted-foreground"
                      title="Curated entry with human-reviewed location notes">Known</span
                    >{/if}
                </div>
                {#if selected.installedSize > 0}<p class="mt-1 text-xs text-muted-foreground">
                    includes {formatBytes(selected.installedSize)} app bundle
                  </p>{/if}
              </div>
            </div>
            <span class="shrink-0 text-sm font-semibold tabular-nums">{formatBytes(selected.totalSize)}</span>
          </div>
          {#if selected.bundleId}<button
              type="button"
              class="mt-1 block truncate font-mono text-xs text-muted-foreground hover:text-foreground"
              title="Copy bundle identifier"
              onclick={() => onCopy(selected.bundleId ?? '')}>{selected.bundleId}</button
            >{/if}
          {#if selected.appPath}<div class="mt-1 flex min-w-0 items-center gap-1">
              <span class="min-w-0 flex-1 truncate font-mono text-xs" title={selected.appPath}>{selected.appPath}</span>
              <button
                class="toolbar-icon"
                type="button"
                onclick={() => selected.appPath && onCopy(selected.appPath)}
                aria-label={`Copy path of ${selected.name}`}
                title="Copy path"><Copy size={14} /></button
              >
            </div>{/if}
          {@render locationList(selected)}
        </div>
      {/if}
      {#if viewMode === 'cards'}
        {#if filtered.length > 0}
          <div class="grid grid-cols-2 gap-3 p-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5" aria-label="Apps">
            {#each filtered as app (app.id)}
              <button
                type="button"
                class="flex min-w-0 flex-col gap-2 rounded-lg border bg-card p-3 text-left hover:ring-1 hover:ring-ring"
                aria-label={`Show details for ${app.name}`}
                onclick={() => (selectedId = app.id)}
              >
                <div class="flex w-full items-start gap-3">
                  {@render appIcon(app)}
                  <div class="min-w-0 flex-1">
                    <strong class="block truncate text-sm" title={app.name}>{app.name}</strong>
                    <div class="mt-1 flex flex-wrap items-center gap-1">
                      <span
                        class="rounded-full border px-2 py-0.5 text-[10px] font-medium text-muted-foreground"
                        >{app.kind === 'app' ? 'App' : 'CLI'}</span
                      >
                      {#if app.source === 'catalog'}<span
                          class="rounded-full border px-2 py-0.5 text-[10px] font-medium text-muted-foreground"
                          title="Curated entry with human-reviewed location notes">Known</span
                        >{/if}
                    </div>
                  </div>
                  <span class="shrink-0 text-sm font-semibold tabular-nums">{formatBytes(app.totalSize)}</span>
                </div>
                <p class="w-full truncate text-xs text-muted-foreground" title={cardSummary(app)}>{cardSummary(app)}</p>
              </button>
            {/each}
          </div>
        {:else}
          <div class="discovery-empty">
            <Search size={28} /><strong>No matching apps</strong>
            <p>Try a different search or size filter.</p>
          </div>
        {/if}
      {:else}
        {#each filtered as app (app.id)}
          <article class="rounded-lg border bg-card p-4">
            <div class="flex items-start justify-between gap-3">
              <div class="min-w-0">
                <div class="flex flex-wrap items-center gap-2">
                  <strong class="text-sm">{app.name}</strong>
                  <span
                    class="rounded-full border px-2 py-0.5 text-[10px] font-medium text-muted-foreground"
                    >{app.kind === 'app' ? 'App' : 'CLI'}</span
                  >
                  {#if app.source === 'catalog'}<span
                      class="rounded-full border px-2 py-0.5 text-[10px] font-medium text-muted-foreground"
                      title="Curated entry with human-reviewed location notes">Known</span
                    >{/if}
                </div>
                {#if app.installedSize > 0}<p class="mt-1 text-xs text-muted-foreground">
                    includes {formatBytes(app.installedSize)} app bundle
                  </p>{/if}
                {#if app.bundleId}<button
                    type="button"
                    class="mt-1 block truncate font-mono text-xs text-muted-foreground hover:text-foreground"
                    title="Copy bundle identifier"
                    onclick={() => onCopy(app.bundleId ?? '')}>{app.bundleId}</button
                  >{/if}
                {#if app.appPath}<div class="mt-1 flex min-w-0 items-center gap-1">
                    <span class="min-w-0 flex-1 truncate font-mono text-xs" title={app.appPath}>{app.appPath}</span>
                    <button
                      class="toolbar-icon"
                      type="button"
                      onclick={() => app.appPath && onCopy(app.appPath)}
                      aria-label={`Copy path of ${app.name}`}
                      title="Copy path"><Copy size={14} /></button
                    >
                  </div>{/if}
              </div>
              <span class="shrink-0 text-sm font-semibold tabular-nums">{formatBytes(app.totalSize)}</span>
            </div>
            {@render locationList(app)}
          </article>
        {/each}
        {#if filtered.length === 0}
          <div class="discovery-empty">
            <Search size={28} /><strong>No matching apps</strong>
            <p>Try a different search or size filter.</p>
          </div>
        {/if}
      {/if}
    {/if}
    {#if !report && !loading && !error}<div class="discovery-empty">
        <RefreshCw size={28} /><strong>No audit yet</strong>
        <p>Refresh to enumerate installed apps and their scattered storage.</p>
      </div>{/if}
    {#if loading}<div class="discovery-loading" role="status">
        <LoaderCircle size={18} class="animate-spin" /> Auditing installed apps…
      </div>{/if}
  </div>
  {#if report}<p class="discovery-footnote">
      Reclaimable classes: {formatBytes(reclaimableTotal)}
      <span class="mt-1 block text-[11px] text-muted-foreground"
        >Classification, not a recommendation — nothing is selected or deleted automatically. Sizes may be lower bounds
        where macOS restricts reads.</span
      >
    </p>{/if}
</section>
