<script lang="ts">
  import { ArrowLeft, RefreshCw } from '@lucide/svelte'
  import { formatBytes } from '../../lib/format'
  import { initialsFor } from '../../lib/app-audit'
  import { ensureIcon, readIcon, subscribeIcons } from '../../lib/app-icon-cache'

  /**
   * V3 scan-header strip (plan §1 decision 1): the compact audit overview
   * that survives the drawer's retirement — app icon, name, audit
   * generation + stale badge, totals, back-to-apps. Rendered above the
   * standard browse workbench for virtual-root sessions.
   */
  interface Props {
    appName: string
    appId: string
    auditId: string
    /** ISO generation time of the pinned audit, if the report is cached. */
    generatedAt?: string | null
    stale?: boolean
    totalBytes?: number
    locationCount?: number | null
    appPath?: string | null
    isCli?: boolean
    rescanning?: boolean
    rescanAvailable?: boolean
    onBack: () => void
    onRescan?: () => void
    onIcon?: (appPath: string) => Promise<string | null>
  }

  let {
    appName,
    appId,
    auditId,
    generatedAt = null,
    stale = false,
    totalBytes = 0,
    locationCount = null,
    appPath = null,
    isCli = false,
    rescanning = false,
    rescanAvailable = true,
    onBack,
    onRescan,
    onIcon,
  }: Props = $props()

  let iconTick = $state(0)
  $effect(() => subscribeIcons(() => (iconTick += 1)))
  $effect(() => {
    if (onIcon && appPath && !isCli) ensureIcon(appPath, onIcon)
  })
  let icon = $derived.by(() => {
    void iconTick
    if (!appPath || isCli) return { status: 'failed' as const, url: null }
    return readIcon(appPath) ?? { status: 'loading' as const, url: null }
  })

  let generationLabel = $derived(
    generatedAt
      ? `Audit ${new Date(generatedAt).toLocaleString()}`
      : `Audit ${auditId}`,
  )
</script>

<header class="app-scan-strip" aria-label={`App scan: ${appName}`}>
  <button type="button" class="strip-back" onclick={onBack} aria-label="Back to apps" title="Back to apps">
    <ArrowLeft size={15} /><span>Apps</span>
  </button>
  {#if icon.status === 'ready' && icon.url}
    <img src={icon.url} alt="" aria-hidden="true" class="size-8 shrink-0 rounded-lg object-contain" />
  {:else if icon.status === 'failed'}
    <div
      class="flex size-8 shrink-0 items-center justify-center rounded-lg bg-muted text-xs font-semibold text-muted-foreground"
      aria-hidden="true">{initialsFor(appName)}</div
    >
  {:else}
    <div class="size-8 shrink-0 animate-pulse rounded-lg bg-muted" aria-hidden="true"></div>
  {/if}
  <div class="min-w-0 flex-1">
    <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5">
      <strong class="truncate text-sm font-semibold" title={appName}>{appName}</strong>
      <span
        class="rounded-full border px-2 py-0.5 text-[10px] font-medium text-muted-foreground"
        title={`App identifier: ${appId}`}>{isCli ? 'CLI' : 'App'}</span
      >
      {#if stale}
        <span
          class="rounded-full border border-amber-500/50 px-2 py-0.5 text-[10px] font-medium text-amber-600 dark:text-amber-400"
          title="The audit refreshed or files changed after cleanup — rescan for current sizes"
          role="status">Stale</span
        >
      {/if}
    </div>
    <p class="mt-0.5 truncate text-[11px] text-muted-foreground" title={generationLabel}>
      {generationLabel}{#if totalBytes > 0} · {formatBytes(totalBytes)}{/if}{#if locationCount !== null}
        · {locationCount} location{locationCount === 1 ? '' : 's'}{/if}
    </p>
  </div>
  {#if onRescan}
    <button
      type="button"
      class="strip-rescan"
      disabled={rescanning || !rescanAvailable}
      onclick={onRescan}
      title={stale ? 'Rescan this app with the current audit generation' : 'Rescan this app'}
    >
      <RefreshCw size={14} class={rescanning ? 'animate-spin' : ''} /><span>{rescanning ? 'Scanning…' : stale ? 'Rescan' : 'Rescan'}</span>
    </button>
  {/if}
</header>

<style>
  .app-scan-strip {
    display: flex;
    align-items: center;
    gap: 0.625rem;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid var(--border);
  }
  .strip-back {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    font-size: 0.8125rem;
    color: var(--muted-foreground);
    border-radius: 0.375rem;
    padding: 0.25rem 0.375rem;
  }
  .strip-back:hover {
    color: var(--foreground);
    background: color-mix(in srgb, var(--muted) 60%, transparent);
  }
  .strip-rescan {
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
    font-size: 0.8125rem;
    border: 1px solid var(--border);
    border-radius: 0.375rem;
    padding: 0.375rem 0.625rem;
    color: var(--foreground);
  }
  .strip-rescan:disabled {
    opacity: 0.55;
  }
  .strip-rescan:not(:disabled):hover {
    background: color-mix(in srgb, var(--muted) 60%, transparent);
  }
</style>
