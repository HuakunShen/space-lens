<script lang="ts">
  import { ArrowRight, FolderSearch, FolderOpen, LoaderCircle, RefreshCw, Search } from '@lucide/svelte'
  import type { AppAuditReport } from '@space-lens/contract'
  import type { ScanStatus, ScanTarget, ScanVolume } from '../../types'
  import type { RecentAppEntry } from '../../lib/app-scan'
  import { formatBytes } from '../../lib/format'
  import { matchesQuery } from '../../lib/app-audit'
  import { initialsFor } from '../../lib/app-audit'
  import { ensureIcon, readIcon, subscribeIcons } from '../../lib/app-icon-cache'
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
    /** V3 cached audit report for the Apps section; never auto-fetched. */
    auditReport?: AppAuditReport | null
    auditLoading?: boolean
    auditError?: string | null
    onRefreshAudit?: () => void
    /** False on hosts without the appAudit capability (section hidden). */
    auditSupported?: boolean
    /** Whether the host can start virtual-root app scans. */
    appScanSupported?: boolean
    scanningAppId?: string | null
    /** Client-side app-scan history incl. A11 provenance/stale flags. */
    recentApps?: RecentAppEntry[]
    onScanApp?: (appId: string) => void
    onForgetApp?: (appId: string) => Promise<void> | void
    onIcon?: (appPath: string) => Promise<string | null>
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
    auditReport = null,
    auditLoading = false,
    auditError = null,
    onRefreshAudit,
    auditSupported = false,
    appScanSupported = false,
    scanningAppId = null,
    recentApps = [],
    onScanApp,
    onForgetApp,
    onIcon = undefined,
  }: Props = $props()
  let selectedId = $state('')
  let customPath = $state('')
  let query = $state('')
  let staleByApp = $derived(new Map((recentApps ?? []).map((entry) => [entry.appId, entry.stale === true])))
  let filteredApps = $derived((auditReport?.apps ?? []).filter((app) => matchesQuery(app, query)))
  let showApps = $derived(
    auditSupported && (onScanApp !== undefined || (recentApps ?? []).length > 0 || auditReport !== null),
  )
  // App icons resolve through the shared cache; failures stay failed.
  let iconTick = $state(0)
  $effect(() => subscribeIcons(() => (iconTick += 1)))
  $effect(() => {
    if (!onIcon) return
    for (const app of filteredApps) {
      if (app.appPath && app.kind !== 'cli') ensureIcon(app.appPath, onIcon)
    }
  })
  function appIconUrl(appPath: string | null, kind: string): string | null {
    void iconTick
    if (!appPath || kind === 'cli') return null
    const cached = readIcon(appPath)
    return cached?.status === 'ready' ? (cached.url ?? null) : null
  }
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
        query={query}
        recentApps={recentApps ?? []}
        onSelectApp={onScanApp}
        onForgetApp={onForgetApp}
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
        <label class="relative mb-4 flex items-center gap-2"
          ><Search size={15} /><Input
            type="search"
            bind:value={query}
            placeholder="Search locations and apps…"
            aria-label="Search locations and apps"
          /></label
        >
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
        {#if showApps}
          <section class="picker-apps" aria-label="Apps">
            <div class="mt-6 mb-2 flex items-center justify-between gap-2">
              <h2 class="text-sm font-semibold">Apps</h2>
              {#if auditReport}
                <div class="flex items-center gap-2">
                  <span class="text-[11px] text-muted-foreground" title={auditReport.generatedAt}
                    >Audited {new Date(auditReport.generatedAt).toLocaleString()}</span
                  >
                  {#if onRefreshAudit}<Button
                      variant="outline"
                      size="sm"
                      disabled={auditLoading || busy}
                      onclick={onRefreshAudit}
                      title="Refresh the app audit (takes a minute or two)"
                      ><RefreshCw size={13} class={auditLoading ? 'animate-spin' : ''} /> Refresh</Button
                    >{/if}
                </div>
              {:else if onRefreshAudit}
                <Button variant="outline" size="sm" disabled={auditLoading || busy} onclick={onRefreshAudit}
                  ><RefreshCw size={13} class={auditLoading ? 'animate-spin' : ''} />
                  {auditLoading ? 'Auditing…' : 'Run audit'}</Button
                >
              {/if}
            </div>
            {#if auditLoading && !auditReport}
              <p class="flex items-center gap-2 text-sm text-muted-foreground" role="status">
                <LoaderCircle size={15} class="animate-spin" /> Auditing installed apps — this takes a minute or two.
              </p>
            {:else if auditError && !auditReport}
              <div class="inline-error" role="alert">
                <p>{auditError}</p>
                {#if onRefreshAudit}<Button variant="outline" size="sm" onclick={onRefreshAudit}>Try again</Button>{/if}
              </div>
            {:else if auditReport}
              {#if auditError}<p class="mb-2 text-xs text-muted-foreground" role="status">
                  Refresh failed — showing the cached audit. {auditError}
                </p>{/if}
              {#if filteredApps.length > 0}
                <ul class="grid gap-2">
                  {#each filteredApps as app (app.id)}
                    {@const icon = appIconUrl(app.appPath, app.kind)}
                    {@const stale = staleByApp.get(app.id) === true}
                    {@const scanning = scanningAppId === app.id}
                    <li
                      class="flex items-center gap-2 rounded-lg border bg-card p-2"
                      class:opacity-70={scanning}
                    >
                      {#if icon}
                        <img src={icon} alt="" aria-hidden="true" class="size-7 shrink-0 rounded-md object-contain" />
                      {:else}
                        <div
                          class="flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-[11px] font-semibold text-muted-foreground"
                          aria-hidden="true">{initialsFor(app.name)}</div
                        >
                      {/if}
                      <div class="min-w-0 flex-1">
                        <div class="flex items-center gap-1.5">
                          <strong class="min-w-0 truncate text-[13px] font-semibold" title={app.name}>{app.name}</strong>
                          {#if stale}<span
                              class="shrink-0 rounded-full border border-amber-500/50 px-1.5 py-px text-[10px] font-medium text-amber-600 dark:text-amber-400"
                              title="Audit refreshed or files changed after cleanup — rescan for current sizes"
                              role="status">Stale</span
                            >{/if}
                        </div>
                        <p class="truncate text-[11px] tabular-nums text-muted-foreground">
                          {formatBytes(app.totalSize)} · {app.locations.length} location{app.locations.length === 1
                            ? ''
                            : 's'}
                        </p>
                      </div>
                      {#if onScanApp}
                        <Button
                          variant="outline"
                          size="sm"
                          disabled={busy || scanning || !appScanSupported}
                          title={appScanSupported
                            ? `Scan ${app.name}`
                            : 'App scans need the desktop host'}
                          onclick={() => onScanApp(app.id)}
                          >{scanning ? 'Scanning…' : 'Scan'}</Button
                        >
                      {/if}
                    </li>
                  {/each}
                </ul>
              {:else}
                <p class="text-sm text-muted-foreground">No matching apps. Try a different search.</p>
              {/if}
            {:else if (recentApps ?? []).length > 0}
              <!-- No cached report yet: fall back to recent-app names only. -->
              <ul class="grid gap-2">
                {#each (recentApps ?? []).filter((entry) => entry.appName.toLowerCase().includes(query.trim().toLowerCase())) as entry (entry.appId)}
                  <li class="flex items-center gap-2 rounded-lg border bg-card p-2">
                    <div
                      class="flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-[11px] font-semibold text-muted-foreground"
                      aria-hidden="true">{initialsFor(entry.appName)}</div
                    >
                    <div class="min-w-0 flex-1">
                      <strong class="block truncate text-[13px] font-semibold" title={entry.appName}>{entry.appName}</strong>
                      <p class="truncate text-[11px] text-muted-foreground">Previously scanned</p>
                    </div>
                    {#if onScanApp}
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={busy || !appScanSupported}
                        onclick={() => onScanApp(entry.appId)}>{scanningAppId === entry.appId ? 'Scanning…' : 'Scan'}</Button
                      >
                    {/if}
                  </li>
                {/each}
              </ul>
              <p class="mt-2 text-[11px] text-muted-foreground">Run the audit for sizes and the full app list.</p>
            {:else}
              <p class="text-sm text-muted-foreground">Run the audit to browse installed apps here.</p>
            {/if}
          </section>
        {/if}
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
