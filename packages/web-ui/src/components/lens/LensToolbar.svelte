<script lang="ts">
  import { FolderPlus, Inbox, PanelLeft, ChartPie, Settings, Menu } from '@lucide/svelte'
  const iconClass =
    'toolbar-icon macos:size-7 macos:rounded-md linux:size-9 linux:rounded-lg linux:bg-(--control) linux:text-foreground linux:shadow-xs windows:size-8 windows:rounded-[4px] focus-visible:ring-2 focus-visible:ring-ring motion-reduce:transition-none'
  interface Props {
    title: string
    logo?: string
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
  let {
    title,
    logo,
    mode = 'browser',
    chromeInset = 'px-4',
    sidebarVisible = true,
    chartVisible = true,
    collectorCount = 0,
    busy = false,
    onNewScan,
    onToggleSidebar,
    onToggleChart,
    onOpenCollector,
    onOpenSettings,
  }: Props = $props()
</script>

<header
  class={[
    'lens-toolbar relative macos:h-14 macos:min-h-14 macos:bg-(--chrome) macos:backdrop-blur-xl windows:h-10 windows:min-h-10 windows:border-0 windows:bg-(--chrome) linux:h-14 linux:min-h-14 linux:gap-3 linux:bg-(--chrome) linux:shadow-xs',
    chromeInset,
  ]}
>
  {#if onToggleSidebar}
    <button
      class={iconClass}
      type="button"
      onclick={onToggleSidebar}
      aria-label="Toggle sidebar"
      aria-pressed={sidebarVisible}
      title="Toggle sidebar"><PanelLeft size={17} /></button
    >
  {/if}
  {#if logo}<img src={logo} alt="" class="size-6 shrink-0 rounded-md macos:hidden linux:hidden" />{/if}
  <div
    class="toolbar-title macos:flex-col macos:items-start macos:gap-0.5 linux:pointer-events-none linux:absolute linux:left-1/2 linux:max-w-[35%] linux:max-md:static linux:max-md:translate-x-0 linux:max-md:items-start linux:max-md:max-w-[25%] linux:-translate-x-1/2 linux:flex-col linux:items-center linux:gap-0.5"
    data-tauri-drag-region
  >
    <strong class="macos:text-sm linux:text-sm windows:hidden" data-tauri-drag-region>{title}</strong>
    <span
      class="macos:text-xs linux:text-xs windows:text-xs windows:font-medium windows:text-foreground"
      data-tauri-drag-region>Space Lens</span
    >
  </div>
  <div class="toolbar-spacer" data-tauri-drag-region></div>
  <div class="toolbar-actions macos:gap-2 linux:gap-2">
    {#if onNewScan}
      <button
        class="toolbar-button windows:hidden macos:h-7 macos:border-transparent macos:bg-transparent linux:h-9 linux:rounded-lg linux:border-transparent linux:bg-primary linux:px-3 linux:text-primary-foreground linux:font-semibold"
        type="button"
        onclick={onNewScan}
        disabled={busy}
        aria-label="New scan"
        title="New scan"><FolderPlus size={16} /><span>New scan</span></button
      >
      <span class="toolbar-divider linux:hidden windows:hidden" aria-hidden="true"></span>
    {/if}
    {#if onOpenCollector}
      <button
        class={[iconClass, 'collector-toggle windows:hidden']}
        type="button"
        onclick={onOpenCollector}
        aria-label={`Review selection (${collectorCount})`}
        title={`Review selection · ${collectorCount} items`}
      >
        <Inbox size={17} />
        {#if collectorCount > 0}<span class="toolbar-count">{collectorCount}</span>{/if}
      </button>
    {/if}
    {#if onToggleChart}
      <button
        class={[iconClass, 'windows:hidden']}
        class:active={chartVisible}
        type="button"
        onclick={onToggleChart}
        aria-label="Toggle storage map"
        aria-pressed={chartVisible}
        title="Toggle storage map"><ChartPie size={17} /></button
      >
    {/if}
    {#if onOpenSettings}
      <button class={iconClass} type="button" onclick={onOpenSettings} aria-label="Settings" title="Settings"
        ><Settings class="linux:hidden" size={17} /><Menu class="hidden linux:block" size={17} /></button
      >
    {/if}
  </div>
</header>
