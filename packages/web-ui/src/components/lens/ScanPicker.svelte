<script lang="ts">
  import { ArrowRight, FolderSearch, FolderOpen } from '@lucide/svelte'
  import type { ScanStatus, ScanTarget, ScanVolume } from '../../types'
  import { formatBytes } from '../../lib/format'
  import { scanTargetPaths } from '../../lib/scan-targets'
  import { Button } from '../ui/button/index'
  import LensToolbar from './LensToolbar.svelte'
  import ScanSidebar from './ScanSidebar.svelte'
  import DriveList from './DriveList.svelte'
  import { Input } from '../ui/input/index'
  import { Progress } from '../ui/progress/index'

  interface Props {
    targets: ScanTarget[]
    /** Mounted volumes for the pre-scan drive list; absent on older hosts. */
    volumes?: ScanVolume[]
    initialPath?: string
    canClose?: boolean
    mode: string
    logo?: string
    busy: boolean
    error: string | null
    status: ScanStatus | null
    onScan: (paths: string[]) => void
    onCancel: () => void
    onSettings?: () => void
    onForget?: (path: string) => Promise<void> | void
    /**
     * Whether the host offers a native folder picker (desktop: the Tauri
     * dialog). The button exists only when this is true — in the browser an
     * absolute path cannot come from a picker, so manual entry stays the way.
     */
    folderPicker?: boolean
    onPickFolder?: () => Promise<string | null>
    /** Where the window chrome sits; defaults follow the mode. */
    chromeInset?: string
  }

  let {
    targets,
    volumes = [],
    initialPath = '',
    canClose = false,
    mode,
    logo,
    busy,
    error,
    status,
    onScan,
    onCancel,
    onSettings,
    onForget,
    folderPicker = false,
    onPickFolder = undefined,
    chromeInset = undefined,
  }: Props = $props()
  let selectedId = $state('')
  let customPath = $state('')
  $effect(() => {
    if (initialPath) {
      customPath = initialPath
      selectedId = 'custom'
    }
  })
  let secondPath = $state('')
  let pickingFolder = $state(false)
  let selectedTarget = $derived(targets.find((target) => target.id === selectedId))
  let startupDisk = $derived(
    targets.some((target) => target.source !== 'recent' && target.path === '/System/Volumes/Data'),
  )
  function targetLabel(target: ScanTarget): string {
    return startupDisk && target.path === '/' ? 'Macintosh HD' : target.label
  }
  let selectedPaths = $derived(
    selectedTarget
      ? scanTargetPaths(selectedTarget, targets)
      : [customPath, secondPath].map((path) => path.trim()).filter(Boolean),
  )
  let canScan = $derived(selectedPaths.length > 0 && !busy)
  let sidebarVisible = $state(true)
  const fieldClass =
    'grid gap-2 macos:grid-cols-[100px_minmax(0,1fr)] macos:items-center macos:gap-3 max-sm:macos:grid-cols-1'
  const labelClass = 'text-sm font-medium macos:text-right macos:text-[13px] max-sm:macos:text-left'

  $effect(() => {
    if (selectedId === 'custom') return
    if (targets.some((target) => target.id === selectedId)) return
    selectedId = targets[0]?.id ?? ''
  })

  function scanSelected() {
    if (!canScan) return
    onScan(selectedPaths)
  }

  async function pickFolder() {
    if (!onPickFolder || pickingFolder) return
    pickingFolder = true
    try {
      const picked = await onPickFolder()
      if (picked) {
        customPath = picked
        selectedId = 'custom'
      }
    } finally {
      pickingFolder = false
    }
  }
</script>

<main class="scan-picker flex h-dvh min-h-0 flex-col bg-background text-foreground">
  <LensToolbar
    title="New scan"
    {mode}
    chromeInset={chromeInset ?? (mode === 'kunkun' || mode === 'desktop' ? 'pl-[88px] pr-4' : 'px-4')}
    {sidebarVisible}
    onToggleSidebar={() => (sidebarVisible = !sidebarVisible)}
    onOpenSettings={onSettings}
    {logo}
  />
  <div class="picker-layout flex min-h-0 flex-1 max-md:flex-col max-md:overflow-auto windows:bg-sidebar">
    {#if sidebarVisible}
      <ScanSidebar
        targets={targets.map((target) => ({ ...target, label: targetLabel(target) }))}
        class="max-md:static max-md:flex-col max-md:max-h-48 max-md:w-full max-md:border-r-0 max-md:border-b max-md:shadow-none"
        selectedPath={selectedTarget?.path}
        customSelected={selectedId === 'custom'}
        {busy}
        {onForget}
        {onSettings}
        onSelect={(target) => (selectedId = target.id)}
        onCustom={() => (selectedId = 'custom')}
      />
    {/if}
    <section
      class="flex min-h-0 min-w-0 flex-1 flex-col overflow-auto p-8 max-md:p-5 windows:rounded-tl-lg windows:border windows:border-border windows:bg-background windows:px-8 linux:bg-background"
    >
      <div class="my-auto w-full macos:mx-auto macos:max-w-2xl linux:mx-auto linux:max-w-3xl windows:max-w-4xl">
        <header class="mb-8 macos:mb-10 macos:text-center linux:text-center">
          <FolderSearch
            class="mb-4 size-10 text-primary macos:mx-auto linux:mx-auto windows:hidden"
            strokeWidth={1.5}
          />
          <h1 class="text-2xl font-semibold tracking-tight macos:text-[23px] windows:text-[28px] linux:text-2xl">
            {status?.state === 'scanning'
              ? 'Building storage map'
              : selectedTarget
                ? targetLabel(selectedTarget)
                : 'Choose a folder to scan'}
          </h1>
          <p class="mt-2 text-sm text-muted-foreground">Explore disk usage and find space to reclaim.</p>
        </header>
        <form
          class="grid gap-6 web:rounded-xl web:border web:bg-card web:p-6 windows:rounded-lg windows:border windows:bg-card windows:p-6 linux:rounded-xl linux:border linux:bg-card linux:p-6"
          onsubmit={(event) => {
            event.preventDefault()
            scanSelected()
          }}
        >
          {#if !selectedTarget}
            <label class={fieldClass}>
              <span class={labelClass}>Folder</span>
              <div class="flex min-w-0 gap-2">
                <Input bind:value={customPath} placeholder="/path/to/folder" disabled={busy} class="flex-1" />
                {#if folderPicker && onPickFolder}
                  <Button variant="outline" onclick={pickFolder} disabled={busy || pickingFolder}
                    ><FolderOpen size={15} />{pickingFolder ? 'Opening…' : 'Choose…'}</Button
                  >
                {/if}
              </div>
            </label>
            <label class={fieldClass}
              ><span class={labelClass}>Second folder</span><Input
                bind:value={secondPath}
                placeholder="Optional second folder"
                disabled={busy}
              /></label
            >
          {:else}
            <div class={fieldClass}>
              <span class={labelClass}>Scan root</span>
              <p
                class="break-all rounded-md border bg-background px-3 py-2 font-mono text-sm macos:border-0 macos:bg-transparent macos:p-0"
              >
                {selectedPaths.join(' · ')}
              </p>
            </div>
          {/if}
          <p class="text-sm leading-relaxed text-muted-foreground macos:ml-[112px] max-sm:macos:ml-0">
            {selectedPaths.includes('/') && startupDisk
              ? 'Local metadata only. Cloud storage and network volumes are skipped.'
              : 'Build a storage map, find large files and review cleanup.'}
          </p>
          <div class="flex justify-end gap-2 border-t pt-4">
            {#if canClose && !busy}<Button variant="outline" onclick={onCancel}>Cancel</Button>{/if}
            <Button type="submit" disabled={!canScan}
              >{busy ? 'Scanning…' : 'Scan'}<ArrowRight class="macos:hidden" size={15} /></Button
            >
          </div>
        </form>
        {#if status?.state === 'scanning'}
          <section class="mt-6 grid gap-3 rounded-lg border bg-card p-5" aria-label="Scan progress" role="status">
            <div class="flex items-center justify-between gap-3">
              <span class="text-sm text-muted-foreground"
                >{#if status.bytesScanned > 0}{formatBytes(status.bytesScanned)} scanned · {status.entriesScanned.toLocaleString()}
                  entries{:else}Measuring files and folders…{/if}</span
              ><Button size="sm" variant="outline" onclick={onCancel}>Stop</Button>
            </div>
            <Progress
              indeterminate={status.progress === null}
              value={status.progress === null ? undefined : Math.round(status.progress * 100)}
            />
            <p class="truncate font-mono text-xs text-muted-foreground">
              {status.currentPath ?? status.label ?? 'Scanning local files…'}
            </p>
          </section>
        {/if}
        {#if error}<div
            role="alert"
            class="mt-4 rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
          >
            {error}
          </div>{/if}
        {#if volumes.length > 0}
          <DriveList
            {volumes}
            {targets}
            {busy}
            onScanDrive={(path) => {
              customPath = path
              selectedId = 'custom'
              scanSelected()
            }}
          />
        {/if}
      </div>
    </section>
  </div>
</main>
