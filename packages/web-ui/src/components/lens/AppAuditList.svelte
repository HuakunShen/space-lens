<script lang="ts">
  import { Copy, LoaderCircle, RefreshCw } from '@lucide/svelte'
  import type { AppAuditReport } from '@space-lens/contract'
  import { formatBytes } from '../../lib/format'
  import { classificationBadge, classificationLabel, purgeableTotal } from '../../lib/app-audit'
  import LensSelect from './LensSelect.svelte'
  import { Button } from '../ui/button/index'

  interface Props {
    report: AppAuditReport | null
    loading: boolean
    error: string | null
    minSize: number
    disabled?: boolean
    onRefresh: () => void
    onMinSize: (value: number) => void
    onCopy: (path: string) => void
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
  }: Props = $props()

  const sizeOptions = [
    { value: '0', label: 'All sizes' },
    { value: String(10 * 1024 ** 2), label: '≥ 10 MB' },
    { value: String(100 * 1024 ** 2), label: '≥ 100 MB' },
    { value: String(1024 ** 3), label: '≥ 1 GB' },
  ]
  let appCount = $derived(report?.apps.length ?? 0)
  let grandTotal = $derived(report?.apps.reduce((total, app) => total + app.totalSize, 0) ?? 0)
  let reclaimable = $derived(report?.apps.flatMap((app) => app.locations) ?? [])
  let reclaimableTotal = $derived(purgeableTotal(reclaimable))
</script>

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
      {#each report.apps as app (app.id)}
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
        </article>
      {/each}
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
