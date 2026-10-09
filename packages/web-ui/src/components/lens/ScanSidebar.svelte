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

  /** Free space, preferring the host's `freeBytes` and falling back to what
   * the filesystem reports as available when a host cannot fill in both. */
  let free = $derived(volume ? (volume.freeBytes || volume.availableBytes) : 0)
  let used = $derived(volume ? Math.max(0, volume.totalBytes - free) : 0)
  let usedFraction = $derived(volume && volume.totalBytes > 0 ? (used / volume.totalBytes) * 100 : 0)
  let capacityLabel = $derived(
    volume
      ? `${scanLabel ?? 'This scan'} volume: ${formatBytes(used)} used of ${formatBytes(volume.totalBytes)}, ${formatBytes(free)} free`
      : '',
  )
  const itemClass =
    'sidebar-item relative macos:h-8 macos:rounded-md macos:text-[13px] macos:font-normal macos:text-foreground windows:h-10 windows:rounded-[4px] windows:text-sm windows:text-foreground windows:font-normal windows:pl-4 windows:data-[selected=true]:before:absolute windows:data-[selected=true]:before:left-0 windows:data-[selected=true]:before:h-4 windows:data-[selected=true]:before:w-[3px] windows:data-[selected=true]:before:rounded-full windows:data-[selected=true]:before:bg-primary linux:h-10 linux:rounded-lg linux:text-sm linux:text-foreground linux:font-medium comfortable:h-11 focus-visible:ring-2 focus-visible:ring-ring'
  const headingClass =
    'macos:text-[11px] macos:mt-5 macos:mb-1 windows:text-xs windows:mt-6 windows:mb-2 linux:text-xs linux:mt-6 linux:mb-2'
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

<aside
  class={[
    'scan-sidebar macos:w-52 macos:bg-sidebar/95 macos:backdrop-blur-xl windows:w-56 windows:border-r-0 windows:bg-sidebar windows:p-3 linux:w-56 linux:bg-sidebar linux:p-3',
    className,
  ]}
  aria-label="Scan locations"
>
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
        {#if target.kind === 'volume'}<HardDrive class="macos:text-primary" size={16} />{:else}<Folder
            class="macos:text-primary"
            size={16}
          />{/if}
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
          class="sidebar-recent macos:rounded-md windows:rounded-[4px] linux:rounded-lg"
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
            ><Folder class="macos:text-primary" size={16} /><span>{target.label}</span></button
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
    <section class="drive-card" aria-label="Disk capacity">
      <div class="drive-head">
        <HardDrive size={17} />
        <div>
          <strong>{scanLabel ?? 'Current scan'}</strong>
          <span>{formatBytes(volume.totalBytes)} total</span>
        </div>
      </div>
      <div class="drive-bar" role="img" aria-label={capacityLabel}>
        <span style={`width: ${usedFraction.toFixed(2)}%`}></span>
      </div>
      <dl class="drive-legend">
        <div><dt><i class="drive-swatch used"></i>Used</dt><dd>{formatBytes(used)}</dd></div>
        <div><dt><i class="drive-swatch free"></i>Free</dt><dd>{formatBytes(free)}</dd></div>
      </dl>
      <div class="drive-scan">
        <i class="drive-dot"></i>
        <div>
          <strong>Scan completed</strong>
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
  <span class="sidebar-footnote macos:text-[11px] windows:hidden linux:hidden">Explore. Select. Review.</span>
</aside>
