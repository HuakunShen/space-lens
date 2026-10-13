<script lang="ts">
  import { Copy, LoaderCircle, X } from '@lucide/svelte'
  import type { AppAuditEntry, TreeSliceNode } from '@space-lens/contract'
  import { formatBytes } from '../../lib/format'
  import { initialsFor, isPathCovered } from '../../lib/app-audit'
  import { ensureIcon, readIcon, subscribeIcons } from '../../lib/app-icon-cache'
  import type { ChartMode, TreemapDensity } from '../../lib/chart-mode'
  import { Button } from '../ui/button/index'
  import DiskChart from './DiskChart.svelte'
  import AppLocationRow from './AppLocationRow.svelte'

  interface Props {
    entry: AppAuditEntry | null
    tree: TreeSliceNode | null
    treeLoading: boolean
    treeError: string | null
    staged: Set<string>
    planning: boolean
    deleting: boolean
    disabled?: boolean
    cleanupAvailable: boolean
    revealAvailable: boolean
    chartMode: ChartMode
    treemapDensity: TreemapDensity
    /** True while the cleanup confirm dialog is open: Esc belongs to it. */
    confirmOpen: boolean
    notice?: string | null
    onClose: () => void
    onToggleStage: (path: string) => void
    onClearStaged: () => void
    onReview: () => void
    onCopy: (path: string) => void
    onReveal: (path: string) => void
    onModeChange: (mode: ChartMode) => void
    onDensityChange: (density: TreemapDensity) => void
    onIcon?: (appPath: string) => Promise<string | null>
  }
  let {
    entry,
    tree,
    treeLoading,
    treeError,
    staged,
    planning,
    deleting,
    disabled = false,
    cleanupAvailable,
    revealAvailable,
    chartMode,
    treemapDensity,
    confirmOpen,
    notice = null,
    onClose,
    onToggleStage,
    onClearStaged,
    onReview,
    onCopy,
    onReveal,
    onModeChange,
    onDensityChange,
    onIcon,
  }: Props = $props()

  let iconTick = $state(0)
  $effect(() => subscribeIcons(() => (iconTick += 1)))
  $effect(() => {
    const appPath = entry?.appPath
    if (onIcon && appPath && entry?.kind !== 'cli') ensureIcon(appPath, onIcon)
  })
  let icon = $derived.by(() => {
    void iconTick
    const appPath = entry?.appPath
    if (!entry || !appPath || entry.kind === 'cli') return { status: 'failed' as const, url: null }
    return readIcon(appPath) ?? { status: 'loading' as const, url: null }
  })

  let busy = $derived(planning || deleting || disabled)
  let sorted = $derived(entry ? [...entry.locations].sort((a, b) => b.size - a.size) : [])
  let stagedCount = $derived(staged.size)
  let stagedBytes = $derived(
    entry ? entry.locations.filter((l) => staged.has(l.path)).reduce((total, l) => total + l.size, 0) : 0,
  )
  let hoveredId = $state<string | null>(null)
  // The chart paints synthetic node ids; map staged audit paths onto them so
  // hovered/staged highlighting follows the drawer selection.
  let collectedIds = $derived(new Set([...staged].map((path) => `apploc:${path}`)))

  let aside = $state<HTMLElement | null>(null)
  $effect(() => {
    function onKey(event: KeyboardEvent): void {
      if (event.key !== 'Escape' || confirmOpen) return
      const focus = document.activeElement
      if (aside && focus && aside.contains(focus)) onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })
</script>

<aside
  class="audit-drawer rounded-lg border bg-card p-4"
  aria-label={entry ? `Details for ${entry.name}` : 'App details'}
  bind:this={aside}
>
  {#if !entry}
    <p class="py-8 text-center text-sm text-muted-foreground">App no longer reported</p>
    <div class="flex justify-center">
      <Button variant="outline" size="sm" onclick={onClose}>← Back to apps</Button>
    </div>
  {:else}
    <div class="mb-3 flex items-center justify-between gap-2">
      <Button variant="ghost" size="sm" onclick={onClose}>← Back to apps</Button>
      <button
        class="toolbar-icon"
        type="button"
        data-drawer-close
        onclick={onClose}
        aria-label="Close details"
        title="Close details"><X size={15} /></button
      >
    </div>
    <div class="flex items-start gap-3">
      {#if icon.status === 'ready' && icon.url}
        <img src={icon.url} alt="" aria-hidden="true" class="size-10 shrink-0 rounded-lg object-contain" />
      {:else if icon.status === 'failed'}
        <div
          class="flex size-10 shrink-0 items-center justify-center rounded-lg bg-muted text-sm font-semibold text-muted-foreground"
          aria-hidden="true">{initialsFor(entry.name)}</div
        >
      {:else}
        <div class="size-10 shrink-0 animate-pulse rounded-lg bg-muted" aria-hidden="true"></div>
      {/if}
      <div class="min-w-0 flex-1">
        <div class="flex items-start justify-between gap-2">
          <strong class="min-w-0 truncate text-base font-semibold" title={entry.name}>{entry.name}</strong>
          <span class="shrink-0 text-sm font-semibold tabular-nums">{formatBytes(entry.totalSize)}</span>
        </div>
        <div class="mt-1 flex flex-wrap items-center gap-1">
          <span class="rounded-full border px-2 py-0.5 text-[10px] font-medium text-muted-foreground"
            >{entry.kind === 'app' ? 'App' : 'CLI'}</span
          >
          {#if entry.source === 'catalog'}<span
              class="rounded-full border px-2 py-0.5 text-[10px] font-medium text-muted-foreground"
              title="Curated entry with human-reviewed location notes">Known</span
            >{/if}
        </div>
        {#if entry.installedSize > 0}<p class="mt-1 text-xs text-muted-foreground">
            includes {formatBytes(entry.installedSize)} app bundle
          </p>{/if}
      </div>
    </div>
    {#if entry.bundleId}<button
        type="button"
        class="mt-2 block w-full truncate text-left font-mono text-xs text-muted-foreground hover:text-foreground"
        title="Copy bundle identifier"
        onclick={() => onCopy(entry.bundleId ?? '')}>{entry.bundleId}</button
      >{/if}
    {#if entry.appPath}<div class="mt-1 flex min-w-0 items-center gap-1">
        <span class="min-w-0 flex-1 truncate font-mono text-xs" title={entry.appPath}>{entry.appPath}</span>
        <button
          class="toolbar-icon"
          type="button"
          onclick={() => entry.appPath && onCopy(entry.appPath)}
          aria-label={`Copy path of ${entry.name}`}
          title="Copy path"><Copy size={14} /></button
        >
      </div>{/if}

    <div class="mt-4">
      {#if treeLoading}
        <div class="app-chart-loading" role="status">
          <LoaderCircle size={16} class="animate-spin" /> Measuring app locations…
        </div>
      {:else if tree}
        <div class="app-chart-slot">
          <DiskChart
            tree={tree}
            focusNode={tree}
            mode={chartMode}
            onModeChange={onModeChange}
            density={treemapDensity}
            onDensityChange={onDensityChange}
            hoveredId={hoveredId}
            collectedIds={collectedIds}
            onHover={(id) => (hoveredId = id)}
            onOpen={() => {}}
            onContext={() => {}}
          />
        </div>
      {:else}
        <p class="app-chart-fallback">Deep sizes unavailable — audit sizes shown.{#if treeError} {treeError}{/if}</p>
      {/if}
    </div>

    {#if notice}<p class="mt-3 text-[11px] text-muted-foreground" role="status">{notice}</p>{/if}

    <ul class="mt-3 space-y-2">
      {#each sorted as location (location.path)}
        <AppLocationRow
          location={location}
          staged={staged.has(location.path)}
          covered={isPathCovered(location.path, staged) && !staged.has(location.path)}
          busy={busy}
          onToggle={onToggleStage}
          onCopy={onCopy}
          onReveal={revealAvailable ? onReveal : undefined}
        />
      {/each}
    </ul>

    {#if cleanupAvailable}
      <div class="app-staged-footer">
        <span class="min-w-0 flex-1 truncate text-xs tabular-nums"
          >{stagedCount} staged · {formatBytes(stagedBytes)}</span
        >
        {#if stagedCount > 0}<Button variant="ghost" size="sm" disabled={busy} onclick={onClearStaged}
            >Clear</Button
          >{/if}
        <Button size="sm" disabled={stagedCount === 0 || planning || deleting || disabled} onclick={onReview}
          >{#if planning}<LoaderCircle size={14} class="animate-spin" />{:else}Review cleanup plan →{/if}</Button
        >
      </div>
    {/if}
  {/if}
</aside>
