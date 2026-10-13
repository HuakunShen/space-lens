<script lang="ts">
  import { LoaderCircle, RefreshCw, Search } from '@lucide/svelte'
  import type { AppAuditEntry, AppAuditReport } from '@space-lens/contract'
  import { formatBytes } from '../../lib/format'
  import {
    classificationBadge,
    classificationLabel,
    matchesQuery,
    purgeableTotal,
  } from '../../lib/app-audit'
  import { initialsFor } from '../../lib/app-audit'
  import { ensureIcon, readIcon, subscribeIcons } from '../../lib/app-icon-cache'
  import LensSelect from './LensSelect.svelte'
  import AppAuditCard from './AppAuditCard.svelte'
  import { Button } from '../ui/button/index'
  import { Input } from '../ui/input/index'

  /**
   * V3 app directory: the audit cards remain the way to browse apps, but
   * selecting an app launches the full scan experience (virtual-root
   * session in the standard workbench), not the retired drawer. Keeps the
   * cards + list modes and the size/search filters.
   */
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
    /** Whether the host can start virtual-root app scans. */
    scanAvailable?: boolean
    /** App id with a scan currently starting (button feedback). */
    scanningAppId?: string | null
    /** Stale audit generations per app id (badge + rescan affordance). */
    staleByApp?: Map<string, boolean> | Record<string, boolean>
    onScanApp?: (entry: AppAuditEntry) => void
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
    scanAvailable = false,
    scanningAppId = null,
    staleByApp,
    onScanApp,
  }: Props = $props()

  const sizeOptions = [
    { value: '0', label: 'All sizes' },
    { value: String(10 * 1024 ** 2), label: '≥ 10 MB' },
    { value: String(100 * 1024 ** 2), label: '≥ 100 MB' },
    { value: String(1024 ** 3), label: '≥ 1 GB' },
  ]
  let query = $state('')
  let viewMode = $state<'cards' | 'list'>('cards')

  let iconTick = $state(0)
  $effect(() => subscribeIcons(() => (iconTick += 1)))
  $effect(() => {
    if (!onIcon) return
    for (const app of filtered) {
      if (app.appPath && app.kind !== 'cli') ensureIcon(app.appPath, onIcon)
    }
  })
  function appIconUrl(app: AppAuditEntry): string | null {
    void iconTick
    if (!app.appPath || app.kind === 'cli') return null
    const cached = readIcon(app.appPath)
    return cached?.status === 'ready' ? (cached.url ?? null) : null
  }

  let appCount = $derived(report?.apps.length ?? 0)
  let grandTotal = $derived(report?.apps.reduce((total, app) => total + app.totalSize, 0) ?? 0)
  let reclaimable = $derived(report?.apps.flatMap((app) => app.locations) ?? [])
  let reclaimableTotal = $derived(purgeableTotal(reclaimable))
  let filtered = $derived((report?.apps ?? []).filter((app) => matchesQuery(app, query)))

  function isStale(appId: string): boolean {
    if (staleByApp instanceof Map) return staleByApp.get(appId) === true
    return (staleByApp as Record<string, boolean> | undefined)?.[appId] === true
  }

  function scanApp(app: AppAuditEntry): void {
    if (disabled || scanningAppId !== null || !scanAvailable) return
    onScanApp?.(app)
  }
</script>

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

<section class="discovery-view audit-root" aria-label="App storage" aria-busy={loading}>
  <div class="audit-main">
    <div class="discovery-heading">
      <div>
        <span class="section-eyebrow">DISCOVER & REVIEW</span>
        <h1>App storage</h1>
        <p>What installed apps and CLI tools keep on this disk — select an app to scan it</p>
      </div>
      {#if report}<div class="discovery-total">
          <strong>{formatBytes(grandTotal)}</strong><span
            >{appCount.toLocaleString()}
            app{appCount === 1 ? '' : 's'} audited</span
          >
        </div>{/if}
    </div>
    <div class="discovery-filters">
      <label class="relative flex min-w-40 flex-1 items-center gap-2"
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
    {#if report}<p class="audit-generation" title={report.auditId ? `Audit generation ${report.auditId}` : undefined}>
        Audited {new Date(report.generatedAt).toLocaleString()}{#if !scanAvailable}
          · app scans need the desktop host{/if}
      </p>{/if}
    <div class="audit-list">
      <div class="discovery-rows">
        {#if report}
          {#if viewMode === 'cards'}
            {#if filtered.length > 0}
              <!-- auto-fill on the card's minimum readable width: narrow hosts
                   collapse to one column, wide ones grow columns for free -->
              <div
                class="grid gap-3 p-4 [grid-template-columns:repeat(auto-fill,minmax(230px,1fr))]"
                aria-label="Apps"
              >
                {#each filtered as app (app.id)}
                  <AppAuditCard
                    entry={app}
                    active={false}
                    stale={isStale(app.id)}
                    scanning={scanningAppId === app.id}
                    scanDisabled={disabled || !scanAvailable}
                    scanTitle={scanAvailable ? `Scan ${app.name}` : 'App scans need the desktop host'}
                    onOpen={() => scanApp(app)}
                    onIcon={onIcon}
                  />
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
              {@const icon = appIconUrl(app)}
              <article class="rounded-lg border bg-card p-4">
                <div class="flex items-start justify-between gap-3">
                  <div class="flex min-w-0 items-start gap-2">
                    {#if icon}
                      <img src={icon} alt="" aria-hidden="true" class="size-8 shrink-0 rounded-lg object-contain" />
                    {:else}
                      <div
                        class="flex size-8 shrink-0 items-center justify-center rounded-lg bg-muted text-xs font-semibold text-muted-foreground"
                        aria-hidden="true">{initialsFor(app.name)}</div
                      >
                    {/if}
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
                        {#if isStale(app.id)}<span
                            class="rounded-full border border-amber-500/50 px-2 py-0.5 text-[10px] font-medium text-amber-600 dark:text-amber-400"
                            title="Audit refreshed or files changed after cleanup — rescan for current sizes"
                            role="status">Stale</span
                          >{/if}
                      </div>
                      {#if app.installedSize > 0}<p class="mt-1 text-xs text-muted-foreground">
                          includes {formatBytes(app.installedSize)} app bundle (audit-time size)
                        </p>{/if}
                      {#if app.bundleId}<button
                          type="button"
                          class="mt-1 block truncate font-mono text-xs text-muted-foreground hover:text-foreground"
                          title="Copy bundle identifier"
                          onclick={() => onCopy(app.bundleId ?? '')}>{app.bundleId}</button
                        >{/if}
                    </div>
                  </div>
                  <span class="shrink-0 text-sm font-semibold tabular-nums">{formatBytes(app.totalSize)}</span>
                </div>
                {@render locationList(app)}
                <div class="mt-3 flex justify-end">
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={disabled || scanningAppId !== null || !scanAvailable}
                    title={scanAvailable ? `Scan ${app.name}` : 'App scans need the desktop host'}
                    onclick={() => scanApp(app)}
                    >{scanningAppId === app.id ? 'Scanning…' : `Scan ${app.name} →`}</Button
                  >
                </div>
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
    </div>
    {#if report}<p class="discovery-footnote">
        Reclaimable classes: {formatBytes(reclaimableTotal)}
        <span class="mt-1 block text-[11px] text-muted-foreground"
          >Classification, not a recommendation — nothing is selected or deleted automatically. Sizes may be lower bounds
          where macOS restricts reads.</span
        >
      </p>{/if}
  </div>
</section>
