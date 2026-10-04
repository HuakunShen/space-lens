<script lang="ts">
  import { FolderPlus, Inbox, PanelLeft, ChartPie, Settings } from '@lucide/svelte'
  interface Props {
    title: string
    mode?: string
    chromeInset?: string
    sidebarVisible?: boolean
    chartVisible?: boolean
    collectorCount?: number
    busy?: boolean
    onNewScan?: () => void
    onToggleSidebar?: () => void
    onToggleChart?: () => void
    onOpenCollector?: () => void
    onOpenSettings?: () => void
  }
  let { title, mode = 'browser', chromeInset = 'px-4', sidebarVisible = true, chartVisible = true,
    collectorCount = 0, busy = false, onNewScan, onToggleSidebar,
    onToggleChart, onOpenCollector, onOpenSettings }: Props = $props()
</script>
<header class={['lens-toolbar', chromeInset]}>
  {#if onToggleSidebar}
    <button class="toolbar-icon" type="button" onclick={onToggleSidebar} aria-label="Toggle sidebar" aria-pressed={sidebarVisible} title="Toggle sidebar"><PanelLeft size={17} /></button>
  {/if}
  <div class="toolbar-title" data-tauri-drag-region>
    <strong data-tauri-drag-region>{title}</strong>
    <span data-tauri-drag-region>Space Lens</span>
  </div>
  <div class="toolbar-spacer" data-tauri-drag-region></div>
  <div class="toolbar-actions">
    {#if onNewScan}
      <button class="toolbar-button" type="button" onclick={onNewScan} disabled={busy} aria-label="New scan" title="New scan"><FolderPlus size={16} /><span>New scan</span></button>
      <span class="toolbar-divider" aria-hidden="true"></span>
    {/if}
    {#if onOpenCollector}
      <button class="toolbar-icon collector-toggle" type="button" onclick={onOpenCollector} aria-label={`Review selection (${collectorCount})`} title={`Review selection · ${collectorCount} items`}>
        <Inbox size={17} />
        {#if collectorCount > 0}<span class="toolbar-count">{collectorCount}</span>{/if}
      </button>
    {/if}
    {#if onToggleChart}
      <button class="toolbar-icon" class:active={chartVisible} type="button" onclick={onToggleChart} aria-label="Toggle storage map" aria-pressed={chartVisible} title="Toggle storage map"><ChartPie size={17} /></button>
    {/if}
    {#if onOpenSettings}
      <button class="toolbar-icon" type="button" onclick={onOpenSettings} aria-label="Settings" title="Settings"><Settings size={17} /></button>
    {/if}
  </div>
</header>
