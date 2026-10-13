<script lang="ts">
  import { tick, untrack } from 'svelte'
  import { Copy, LoaderCircle, RefreshCw, Search } from '@lucide/svelte'
  import type {
    AppAuditEntry,
    AppAuditReport,
    AppCleanupPlan,
    CleanupOutcome,
  } from '@space-lens/contract'
  import { formatBytes } from '../../lib/format'
  import {
    appTreeSlice,
    classificationBadge,
    classificationLabel,
    matchesQuery,
    purgeableTotal,
    validateStagedAgainstReport,
  } from '../../lib/app-audit'
  import { DEFAULT_CHART_MODE, DEFAULT_TREEMAP_DENSITY } from '../../lib/chart-mode'
  import type { ChartMode, TreemapDensity } from '../../lib/chart-mode'
  import LensSelect from './LensSelect.svelte'
  import AppAuditCard from './AppAuditCard.svelte'
  import AppAuditDrawer from './AppAuditDrawer.svelte'
  import AppCleanupDialog from './AppCleanupDialog.svelte'
  import { Button } from '../ui/button/index'
  import { Input } from '../ui/input/index'

  interface Props {
    report: AppAuditReport | null
    loading: boolean
    error: string | null
    minSize: number
    disabled?: boolean
    cleanupAvailable?: boolean
    revealAvailable?: boolean
    onRefresh: () => void
    onMinSize: (value: number) => void
    onCopy: (path: string) => void
    onIcon?: (appPath: string) => Promise<string | null>
    onReveal?: (appId: string, path: string) => void
    onCleanupPlan?: (appId: string, paths: string[]) => Promise<AppCleanupPlan>
    onCleanupExecute?: (planId: string) => Promise<CleanupOutcome>
  }
  let {
    report,
    loading,
    error,
    minSize,
    disabled = false,
    cleanupAvailable = false,
    revealAvailable = false,
    onRefresh,
    onMinSize,
    onCopy,
    onIcon,
    onReveal,
    onCleanupPlan,
    onCleanupExecute,
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
  // Staging is per-app and never pre-selected: nothing is staged until the
  // user checks a location row. There is no "select all" affordance.
  let staged = $state<Map<string, Set<string>>>(new Map())
  let prunedNotice = $state<string | null>(null)

  let dialogOpen = $state(false)
  let dialogPlan = $state<AppCleanupPlan | null>(null)
  let dialogOutcome = $state<CleanupOutcome | null>(null)
  let dialogError = $state<string | null>(null)
  let appPlanning = $state(false)
  let appDeleting = $state(false)

  let chartMode = $state<ChartMode>(DEFAULT_CHART_MODE)
  let treemapDensity = $state<TreemapDensity>(DEFAULT_TREEMAP_DENSITY)

  let appCount = $derived(report?.apps.length ?? 0)
  let grandTotal = $derived(report?.apps.reduce((total, app) => total + app.totalSize, 0) ?? 0)
  let reclaimable = $derived(report?.apps.flatMap((app) => app.locations) ?? [])
  let reclaimableTotal = $derived(purgeableTotal(reclaimable))
  let filtered = $derived((report?.apps ?? []).filter((app) => matchesQuery(app, query)))
  let selected = $derived(report?.apps.find((app) => app.id === selectedId) ?? null)
  let selectedStaged = $derived(selectedId ? (staged.get(selectedId) ?? new Set<string>()) : new Set<string>())
  let tree = $derived(selected ? appTreeSlice(selected) : null)
  let dialogStaged = $derived(
    selected
      ? selected.locations
          .filter((location) => selectedStaged.has(location.path))
          .map((location) => ({ path: location.path, size: location.size }))
      : [],
  )
  let dialogStagedTotal = $derived(dialogStaged.reduce((total, row) => total + row.size, 0))

  // Refresh swaps the report without clearing it, so re-validate staged
  // paths against the new generation: vanished apps/paths drop out.
  $effect(() => {
    const current = report
    untrack(() => {
      if (!current) return
      let before = 0
      for (const paths of staged.values()) before += paths.size
      const validated = validateStagedAgainstReport(staged, current)
      let after = 0
      for (const paths of validated.values()) after += paths.size
      if (after !== before || validated.size !== staged.size) {
        staged = validated
        prunedNotice =
          after < before
            ? `${before - after} staged path${before - after === 1 ? '' : 's'} no longer reported — removed.`
            : null
      } else {
        prunedNotice = null
      }
    })
  })

  function openCard(id: string): void {
    if (selectedId === id) {
      closeDrawer()
      return
    }
    selectedId = id
    // Swapping apps closes the confirm dialog; staging is per-app and stays.
    dialogOpen = false
    dialogPlan = null
    dialogOutcome = null
    dialogError = null
    void tick().then(() => {
      document.querySelector<HTMLElement>('[data-drawer-close]')?.focus()
    })
  }

  function closeDrawer(): void {
    const id = selectedId
    selectedId = null
    if (id) {
      void tick().then(() => {
        document.getElementById(`app-card-${id}`)?.focus()
      })
    }
  }

  function toggleStage(path: string): void {
    const id = selectedId
    if (!id || appPlanning || appDeleting) return
    const next = new Map(staged)
    const paths = new Set(next.get(id) ?? [])
    if (paths.has(path)) paths.delete(path)
    else paths.add(path)
    if (paths.size === 0) next.delete(id)
    else next.set(id, paths)
    staged = next
    prunedNotice = null
  }

  function clearStaged(): void {
    const id = selectedId
    if (!id) return
    const next = new Map(staged)
    next.delete(id)
    staged = next
    prunedNotice = null
  }

  function openReview(): void {
    dialogPlan = null
    dialogOutcome = null
    dialogError = null
    dialogOpen = true
  }

  function resetDialog(): void {
    dialogOpen = false
    dialogPlan = null
    dialogOutcome = null
    dialogError = null
  }

  async function planFromStaged(): Promise<void> {
    const id = selectedId
    if (!id || !onCleanupPlan || appPlanning || appDeleting) return
    const paths = [...(staged.get(id) ?? [])]
    if (paths.length === 0) return
    appPlanning = true
    dialogError = null
    try {
      dialogPlan = await onCleanupPlan(id, paths)
    } catch (planError) {
      dialogError = planError instanceof Error ? planError.message : String(planError)
    } finally {
      appPlanning = false
    }
  }

  async function confirmDelete(): Promise<void> {
    const plan = dialogPlan
    if (!plan || !onCleanupExecute || appDeleting) return
    appDeleting = true
    dialogError = null
    try {
      dialogOutcome = await onCleanupExecute(plan.planId)
      const trashed = new Set(dialogOutcome.trashed.map((item) => item.path))
      const next = new Map<string, Set<string>>()
      for (const [appId, paths] of staged) {
        const kept = new Set([...paths].filter((path) => !trashed.has(path)))
        if (kept.size > 0) next.set(appId, kept)
      }
      staged = next
    } catch (executeError) {
      dialogError = executeError instanceof Error ? executeError.message : String(executeError)
    } finally {
      appDeleting = false
    }
  }

  function backToReview(): void {
    dialogPlan = null
    dialogOutcome = null
    dialogError = null
  }

  function removeStaged(path: string): void {
    const id = selectedId
    if (!id) return
    const next = new Map(staged)
    const paths = new Set(next.get(id) ?? [])
    paths.delete(path)
    if (paths.size === 0) next.delete(id)
    else next.set(id, paths)
    staged = next
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
  <!-- Master-detail: the split is the top-level row. The left column owns the heading,
       filters, grid, and footnote, so the search bar ends before the rail; the right
       rail is the drawer spanning the full height of the view. -->
  <div class="audit-split">
    <div class="audit-main">
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
                    <AppAuditCard entry={app} active={app.id === selectedId} onOpen={openCard} onIcon={onIcon} />
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
      </div>
      {#if report}<p class="discovery-footnote">
          Reclaimable classes: {formatBytes(reclaimableTotal)}
          <span class="mt-1 block text-[11px] text-muted-foreground"
            >Classification, not a recommendation — nothing is selected or deleted automatically. Sizes may be lower bounds
            where macOS restricts reads.</span
          >
        </p>{/if}
    </div>
    {#if selectedId !== null}
      <AppAuditDrawer
        entry={selected}
        tree={tree}
        treeLoading={false}
        treeError={null}
        staged={selectedStaged}
        planning={appPlanning}
        deleting={appDeleting}
        disabled={disabled}
        {cleanupAvailable}
        {revealAvailable}
        {chartMode}
        {treemapDensity}
        confirmOpen={dialogOpen}
        notice={prunedNotice}
        onClose={closeDrawer}
        onToggleStage={toggleStage}
        onClearStaged={clearStaged}
        onReview={openReview}
        onCopy={onCopy}
        onReveal={(path) => {
          const id = selectedId
          if (id) onReveal?.(id, path)
        }}
        onModeChange={(mode) => (chartMode = mode)}
        onDensityChange={(density) => (treemapDensity = density)}
        onIcon={onIcon}
      />
    {/if}
  </div>
  <AppCleanupDialog
    open={dialogOpen}
    appName={selected?.name ?? ''}
    staged={dialogStaged}
    stagedTotal={dialogStagedTotal}
    plan={dialogPlan}
    outcome={dialogOutcome}
    error={dialogError}
    planning={appPlanning}
    deleting={appDeleting}
    disabled={disabled}
    {cleanupAvailable}
    onClose={resetDialog}
    onRemove={removeStaged}
    onPlan={() => void planFromStaged()}
    onConfirm={() => void confirmDelete()}
    onBackToReview={backToReview}
    onClear={clearStaged}
  />
</section>
