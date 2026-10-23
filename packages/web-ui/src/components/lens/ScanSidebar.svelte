<script lang="ts">
  import {
    Folder,
    FolderPlus,
    HardDrive,
    X,
    ChartPie,
    FileSearch,
    Boxes,
    GitBranch,
    AppWindow,
    Settings,
    RefreshCw,
  } from '@lucide/svelte'
  import type { Snippet } from 'svelte'
  import type { ScanStatus, ScanTarget, ScanVolume } from '../../types'
  import type { RecentAppEntry } from '../../lib/app-scan'
  import { formatBytes } from '../../lib/format'
  import { scanVolumeLabel, volumeCapacity } from '../../lib/scan-volume'

  interface Props {
    targets: ScanTarget[]
    onSettings?: () => void
    class?: string
    view?: 'browse' | 'large-files' | 'caches' | 'gitignored' | 'apps'
    discoveryAvailable?: boolean
    gitignoredAvailable?: boolean
    appAuditAvailable?: boolean
    onView?: (view: NonNullable<Props['view']>) => void
    selectedPath?: string | null
    customSelected?: boolean
    busy?: boolean
    onSelect: (target: ScanTarget) => void
    onCustom: () => void
    onForget?: (path: string) => Promise<void> | void
    /** V3 live app-scan sessions for the app switcher (§2.3). */
    appScans?: ScanStatus[]
    activeScanId?: string | null
    onSwitchScan?: (scanId: string) => void
    /** Scan ids with a stale audit generation (badge + rescan affordance). */
    staleScanIds?: Set<string> | string[]
    /** Client-side app-scan history (badges survive reload via A11 metadata). */
    recentApps?: RecentAppEntry[]
    onSelectApp?: (appId: string) => void
    onForgetApp?: (appId: string) => Promise<void> | void
    /** A6: switching sessions is blocked while a cleanup executes. */
    switchingBlocked?: boolean
    /** Picker search text; narrows locations, recents and recent apps. */
    query?: string
    /** The volume the current scan lives on, for the capacity card. */
    volume?: ScanVolume | null
    /** What the last scan measured, shown under the capacity bar. */
    scanBytes?: number
    scanEntries?: number
    scanLabel?: string | null
    rescanning?: boolean
    onRescan?: () => void
    /** Host-supplied notices that sit above the footnote, e.g. a permission hint. */
    footer?: Snippet
  }

  let {
    targets,
    onSettings,
    class: className,
    view = 'browse',
    discoveryAvailable = false,
    gitignoredAvailable = discoveryAvailable,
    appAuditAvailable = false,
    onView,
    selectedPath = null,
    customSelected = false,
    busy = false,
    onSelect,
    onCustom,
    onForget,
    appScans = [],
    activeScanId = null,
    onSwitchScan,
    staleScanIds,
    recentApps = [],
    onSelectApp,
    onForgetApp,
    switchingBlocked = false,
    query = '',
    volume = null,
    scanBytes = 0,
    scanEntries = 0,
    scanLabel = null,
    rescanning = false,
    onRescan,
    footer,
  }: Props = $props()

  let capacity = $derived(volumeCapacity(volume))
  let driveName = $derived(volume ? scanVolumeLabel(volume, targets) : '')
  /** Picker search narrows locations, recents and recent apps; sessions stay. */
  let queryValue = $derived((query ?? '').trim().toLowerCase())
  let staleSet = $derived(staleScanIds instanceof Set ? staleScanIds : new Set(staleScanIds ?? []))
  let liveAppScans = $derived((appScans ?? []).filter((scan) => scan.app))
  function appScanLabel(scan: ScanStatus): string {
    return scan.label ?? scan.app?.appName ?? scan.app?.appId ?? scan.scanId
  }
  let capacityLabel = $derived(
    capacity
      ? `${driveName}: ${formatBytes(capacity.used)} used of ${formatBytes(capacity.total)}, ${formatBytes(capacity.free)} free`
      : `${driveName}: capacity unavailable`,
  )
  const itemClass = 'sidebar-item'
  const headingClass = 'sidebar-heading'
  let forgetting = $state<string | null>(null)
  let recent = $derived(
    targets
      .filter((target) => target.source === 'recent')
      .filter((target) => queryValue === '' || target.label.toLowerCase().includes(queryValue) || target.path.toLowerCase().includes(queryValue)),
  )
  let locations = $derived(
    targets
      .filter((target) => target.source !== 'recent')
      .filter((target) => queryValue === '' || target.label.toLowerCase().includes(queryValue) || target.path.toLowerCase().includes(queryValue)),
  )
  let filteredRecentApps = $derived(
    (recentApps ?? []).filter(
      (app) => queryValue === '' || app.appName.toLowerCase().includes(queryValue),
    ),
  )

  async function forget(path: string) {
    if (!onForget) return
    forgetting = path
    try {
      await onForget(path)
    } finally {
      forgetting = null
    }
  }

  async function forgetApp(appId: string) {
    if (!onForgetApp) return
    forgetting = appId
    try {
      await onForgetApp(appId)
    } finally {
      forgetting = null
    }
  }
</script>

<aside class={['scan-sidebar', className]} aria-label="Scan locations">
  <div class="sidebar-scroll">
    {#if onView}
      <h2 class={headingClass}>Workspace</h2>
      {#each [{ id: 'browse', label: 'Browse', icon: ChartPie }, { id: 'large-files', label: 'Large files', icon: FileSearch }, { id: 'caches', label: 'Developer cleanup', icon: Boxes }, { id: 'apps', label: 'App storage', icon: AppWindow }, { id: 'gitignored', label: 'Gitignored space', icon: GitBranch }] as option}
        <button
          class={itemClass}
          data-selected={view === option.id}
          class:selected={view === option.id}
          type="button"
          disabled={option.id !== 'browse' &&
            (option.id === 'apps' ? !appAuditAvailable : (!discoveryAvailable || (option.id === 'gitignored' && !gitignoredAvailable)))}
          aria-current={view === option.id ? 'page' : undefined}
          title={option.id === 'gitignored' && !gitignoredAvailable
            ? 'Scan a local project folder to classify its ignore rules'
            : option.id === 'apps' && !appAuditAvailable
              ? 'App storage audit is unavailable on this host'
              : option.id !== 'browse' && !discoveryAvailable
              ? 'Discovery is unavailable for this scan'
              : option.label}
          onclick={() =>
            onView?.(
              option.id === 'large-files'
                ? 'large-files'
                : option.id === 'caches'
                  ? 'caches'
                  : option.id === 'apps'
                    ? 'apps'
                    : option.id === 'gitignored'
                    ? 'gitignored'
                    : 'browse',
            )}
        >
          <option.icon size={16} /><span>{option.label}</span>
        </button>
      {/each}
    {/if}
    <h2 class={headingClass}>Locations</h2>
    {#each locations as target (target.id)}
      <button
        class={itemClass}
        data-selected={!customSelected && selectedPath === target.path}
        class:selected={!customSelected && selectedPath === target.path}
        type="button"
        disabled={busy}
        aria-pressed={!customSelected && selectedPath === target.path}
        onclick={() => onSelect(target)}
        title={target.path}
      >
        {#if target.kind === 'volume'}<HardDrive size={16} />{:else}<Folder size={16} />{/if}
        <span>{target.label}</span>
      </button>
    {/each}
    <button
      class={itemClass}
      data-selected={customSelected}
      class:selected={customSelected}
      type="button"
      disabled={busy}
      onclick={onCustom}
      aria-pressed={customSelected}
    >
      <FolderPlus size={16} /><span>New location</span>
    </button>
    {#if recent.length > 0}
      <h2 class={headingClass}>Recent scans</h2>
      {#each recent as target (target.id)}
        <div
          class="sidebar-recent"
          class:selected={!customSelected && selectedPath === target.path}
        >
          <button
            class={itemClass}
            data-selected={!customSelected && selectedPath === target.path}
            type="button"
            disabled={busy}
            onclick={() => onSelect(target)}
            title={target.path}
            aria-pressed={!customSelected && selectedPath === target.path}
            ><Folder size={16} /><span>{target.label}</span></button
          >
          {#if onForget}
            <button
              class="sidebar-forget"
              type="button"
              disabled={busy || forgetting === target.path}
              onclick={() => void forget(target.path)}
              aria-label={`Forget ${target.label}`}
              title="Remove from recent scans"><X size={12} /></button
            >
          {/if}
        </div>
      {/each}
    {/if}
    {#if onSwitchScan && liveAppScans.length > 0}
      <h2 class={headingClass}>App scans</h2>
      {#each liveAppScans as scan (scan.scanId)}
        {@const stale = staleSet.has(scan.scanId)}
        <button
          class={itemClass}
          data-selected={activeScanId === scan.scanId}
          class:selected={activeScanId === scan.scanId}
          type="button"
          disabled={busy || switchingBlocked}
          onclick={() => onSwitchScan(scan.scanId)}
          title={switchingBlocked
            ? 'Finish the running cleanup before switching scans'
            : stale
              ? `${appScanLabel(scan)} — audit refreshed, rescan available`
              : appScanLabel(scan)}
          aria-pressed={activeScanId === scan.scanId}
          ><AppWindow size={16} /><span>{appScanLabel(scan)}</span>{#if stale}<span
              class="stale-dot"
              title="Audit refreshed — rescan available"
              aria-label="Stale audit generation"
            ></span>{/if}</button
        >
      {/each}
    {/if}
    {#if onSelectApp && filteredRecentApps.length > 0}
      <h2 class={headingClass}>Recent apps</h2>
      {#each filteredRecentApps as app (app.appId)}
        <div class="sidebar-recent" class:selected={false}>
          <button
            class={itemClass}
            data-selected={false}
            type="button"
            disabled={busy}
            onclick={() => onSelectApp(app.appId)}
            title={app.stale ? `${app.appName} — stale, rescan available` : app.appName}
            ><AppWindow size={16} /><span>{app.appName}</span>{#if app.stale}<span
                class="stale-dot"
                title="Stale — rescan available"
                aria-label="Stale audit generation"
              ></span>{/if}</button
          >
          {#if onForgetApp}
            <button
              class="sidebar-forget"
              type="button"
              disabled={busy || forgetting === app.appId}
              onclick={() => void forgetApp(app.appId)}
              aria-label={`Forget ${app.appName}`}
              title="Remove from recent apps"><X size={12} /></button
            >
          {/if}
        </div>
      {/each}
    {/if}
  </div>
  {#if onSettings}<button class={[itemClass, 'mt-3 hidden windows:flex linux:flex']} type="button" onclick={onSettings}
      ><Settings size={16} /><span>Settings</span></button
    >{/if}
  {#if volume}
    <section class="drive-card" aria-label="Disk capacity" title={volume.path}>
      <div class="drive-head">
        <HardDrive size={17} />
        <div>
          <strong>{driveName}</strong>
          <span>{capacity ? `${formatBytes(capacity.total)} total` : 'Capacity unavailable'}</span>
        </div>
      </div>
      {#if capacity}
        <div class="drive-usage"><span>Disk usage</span><strong>{capacity.usedPercent.toFixed(1)}%</strong></div>
        <div class="drive-bar" role="img" aria-label={capacityLabel}>
          <span style={`width: ${capacity.usedPercent.toFixed(2)}%`}></span>
        </div>
        <dl class="drive-legend">
          <div><dt><i class="drive-swatch used"></i>Used</dt><dd>{formatBytes(capacity.used)}</dd></div>
          <div><dt><i class="drive-swatch free"></i>Free</dt><dd>{formatBytes(capacity.free)}</dd></div>
        </dl>
      {/if}
      <div class="drive-scan" title={scanLabel ?? undefined}>
        <i class="drive-dot"></i>
        <div>
          <strong>{rescanning ? 'Rescanning…' : 'Last scan'}</strong>
          <span>{scanEntries.toLocaleString()} items · {formatBytes(scanBytes)} scanned</span>
        </div>
      </div>
      {#if onRescan}
        <button class="drive-rescan" type="button" disabled={rescanning} onclick={onRescan}>
          <RefreshCw size={14} /><span>{rescanning ? 'Rescanning…' : 'Rescan'}</span>
        </button>
      {/if}
    </section>
  {/if}
  {#if footer}<div class="sidebar-notice">{@render footer()}</div>{/if}
  <span class="sidebar-footnote windows:hidden linux:hidden">Explore. Select. Review.</span>
</aside>
