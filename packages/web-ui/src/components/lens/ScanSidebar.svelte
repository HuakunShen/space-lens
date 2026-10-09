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
    Settings,
    RefreshCw,
  } from '@lucide/svelte'
  import type { ScanTarget, ScanVolume } from '../../types'
  import { formatBytes } from '../../lib/format'
  import { scanVolumeLabel, volumeCapacity } from '../../lib/scan-volume'

  interface Props {
    targets: ScanTarget[]
    onSettings?: () => void
    class?: string
    view?: 'browse' | 'large-files' | 'caches' | 'gitignored'
    discoveryAvailable?: boolean
    gitignoredAvailable?: boolean
    onView?: (view: NonNullable<Props['view']>) => void
    selectedPath?: string | null
    customSelected?: boolean
    busy?: boolean
    onSelect: (target: ScanTarget) => void
    onCustom: () => void
    onForget?: (path: string) => Promise<void> | void
    /** The volume the current scan lives on, for the capacity card. */
    volume?: ScanVolume | null
    /** What the last scan measured, shown under the capacity bar. */
    scanBytes?: number
    scanEntries?: number
    scanLabel?: string | null
    rescanning?: boolean
    onRescan?: () => void
  }

  let {
    targets,
    onSettings,
    class: className,
    view = 'browse',
    discoveryAvailable = false,
    gitignoredAvailable = discoveryAvailable,
    onView,
    selectedPath = null,
    customSelected = false,
    busy = false,
    onSelect,
    onCustom,
    onForget,
    volume = null,
    scanBytes = 0,
    scanEntries = 0,
    scanLabel = null,
    rescanning = false,
    onRescan,
  }: Props = $props()

  let capacity = $derived(volumeCapacity(volume))
  let driveName = $derived(volume ? scanVolumeLabel(volume, targets) : '')
  let capacityLabel = $derived(
    capacity
      ? `${driveName}: ${formatBytes(capacity.used)} used of ${formatBytes(capacity.total)}, ${formatBytes(capacity.free)} free`
      : `${driveName}: capacity unavailable`,
  )
  const itemClass = 'sidebar-item'
  const headingClass = 'sidebar-heading'
  let forgetting = $state<string | null>(null)
  let recent = $derived(targets.filter((target) => target.source === 'recent'))
  let locations = $derived(targets.filter((target) => target.source !== 'recent'))

  async function forget(path: string) {
    if (!onForget) return
    forgetting = path
    try {
      await onForget(path)
    } finally {
      forgetting = null
    }
  }
</script>

<aside class={['scan-sidebar', className]} aria-label="Scan locations">
  <div class="sidebar-scroll">
    {#if onView}
      <h2 class={headingClass}>Workspace</h2>
      {#each [{ id: 'browse', label: 'Browse', icon: ChartPie }, { id: 'large-files', label: 'Large files', icon: FileSearch }, { id: 'caches', label: 'Developer cleanup', icon: Boxes }, { id: 'gitignored', label: 'Gitignored space', icon: GitBranch }] as option}
        <button
          class={itemClass}
          data-selected={view === option.id}
          class:selected={view === option.id}
          type="button"
          disabled={option.id !== 'browse' &&
            (!discoveryAvailable || (option.id === 'gitignored' && !gitignoredAvailable))}
          aria-current={view === option.id ? 'page' : undefined}
          title={option.id === 'gitignored' && !gitignoredAvailable
            ? 'Scan a local project folder to classify its ignore rules'
            : option.id !== 'browse' && !discoveryAvailable
              ? 'Discovery is unavailable for this scan'
              : option.label}
          onclick={() =>
            onView?.(
              option.id === 'large-files'
                ? 'large-files'
                : option.id === 'caches'
                  ? 'caches'
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
  <span class="sidebar-footnote windows:hidden linux:hidden">Explore. Select. Review.</span>
</aside>
