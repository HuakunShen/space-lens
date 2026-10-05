<script lang="ts">
  import { onMount } from 'svelte'
  import { base } from '$app/paths'
  import {
    BreadcrumbBar,
    ChildList,
    CollectorPanel,
    ConnectionPanel,
    ScanPicker,
    StateBanner,
    StatusBar,
    SunburstChart,
    LensSelect, LensToolbar, ScanSidebar, DiscoveryList, AppearancePanel, ScanCoveragePanel, Button, Input, formatBytes, formatNodeName,
  } from '@space-lens/web-ui'
  import type { ScanTarget, TreeNodeSummary } from '@space-lens/web-ui/types'
  import type { CleanupPlan, CleanupOutcome, DiscoveryKind, DiscoveryPage, DiscoveryItem } from '@space-lens/contract'
  import { setMode, userPrefersMode } from 'mode-watcher'
  import { addSelection, selectedAncestor } from '../lib/selection'
  import { ACTIVE_SCAN_KEY, parseActiveScan } from '../lib/resume'
  import { APPEARANCE_KEY, parseAppearance, resolveStyle } from '../lib/appearance'
  import { checkForAppUpdate, currentAppVersion, installAppUpdate } from '../lib/updater'
  import type { UpdateStatus } from '../lib/updater'
  import { ServiceError } from '@space-lens/client'
  import {
    clearToken,
    describeError,
    loadRecentTargets,
    loadRoot,
    loadToken,
    rememberBaseUrl,
    rememberRecentTarget,
    resolveBaseUrl,
    saveToken,
    startEventStream,
    stripTicketFromLocation,
    ticketFromLocation,
    workbench,
  } from '../lib/workbench.svelte'
  import {
    cleanupAvailable as cleanupGate,
    discoveryAvailable as discoveryGate,
    gitignoredAvailable as gitignoredGate,
  } from '../lib/gates'
  import { startDesktopEventStream } from '../lib/desktop-events'

  let ticketInput = $state('')
  let passwordInput = $state('')
  let explicitUrlInput = $state('')
  let collectorOpen = $state(false)
  let settingsOpen = $state(false)
  let sidebarVisible = $state(true)
  let chartVisible = $state(true)
  let pickerOpen = $state(false)
  let pickerPath = $state('')
  let startingScan = $state(false)
  let recentTargets = $state(loadRecentTargets())
  let appearance = $state(parseAppearance(null))
  let appVersion = $state<string | null>(null)
  let updateStatus = $state<UpdateStatus>({ state: 'idle' })
  if (__SPACLENS_DESKTOP__) void currentAppVersion().then(version => (appVersion = version))
  let preferencesReady = $state(false)
  let view = $state<'browse' | DiscoveryKind>('browse')
  let discoveryPage = $state<DiscoveryPage | null>(null)
  let discoveryItems = $state<DiscoveryItem[]>([])
  let discoveryLoading = $state(false)
  let discoveryError = $state<string | null>(null)
  let minimumSize = $state(10 * 1024 ** 2)
  let discoverySequence = 0
  let browseSearch = $state('')
  let browseSort = $state<'size' | 'name' | 'path'>('size')
  const sortOptions = [{ value: 'size', label: 'Largest first' }, { value: 'name', label: 'Name' }, { value: 'path', label: 'Path' }]
  const isMacScan = $derived(workbench.status?.coverage?.protection === 'macos-no-materialization')
  const displayFocus = $derived(workbench.slice ? { ...workbench.slice.focusNode, name: formatNodeName(workbench.slice.focusNode, isMacScan) } : null)
  const scannedLocationOptions = $derived(workbench.status?.rootIds.map((rootId, index) => ({ value: rootId, label: scannedPaths[index] === '/' ? isMacScan ? 'Macintosh HD' : 'Filesystem' : scannedPaths[index] === '/System/Volumes/Data' ? 'Data' : scannedPaths[index]?.split(/[\\/]/).at(-1) || `Location ${index + 1}` })) ?? [])
  let planning = $state(false)
  let cleanupPlan = $state<CleanupPlan | null>(null)
  let cleanupOutcome = $state<CleanupOutcome | null>(null)
  let cleanupError = $state<string | null>(null)
  let notice = $state<string | null>(null)
  let scannedPaths = $state<string[]>([])
  // Desktop and browser share the same availability gates: both backends
  // derive discovery from the protected scan report, so coverage no longer
  // disables the discovery views here.
  const discoveryAvailable = $derived(discoveryGate({ capabilities: workbench.capabilities, service: workbench.service }))
  const gitignoredAvailable = $derived(gitignoredGate({ capabilities: workbench.capabilities, service: workbench.service, coverage: workbench.status?.coverage, scannedPaths }))
  const cleanupAvailable = $derived(cleanupGate({ capabilities: workbench.capabilities, desktop: __SPACLENS_DESKTOP__, coverage: workbench.status?.coverage }))
  const selectedItems = $derived(workbench.items.filter(item => item.name.toLowerCase().includes(browseSearch.toLowerCase())))
  const selectableShown = $derived(selectedItems.filter(item => item.scanState !== 'skipped' && item.scanState !== 'partial'))
  const coveredIds = $derived(new Set(workbench.items.filter(item => selectedAncestor(workbench.collector, item.path)).map(item => item.id)))
  const automaticStyle = resolveStyle('auto', __SPACLENS_DESKTOP__, typeof navigator !== 'undefined' ? navigator.userAgent : '')

  $effect(() => {
    if (!preferencesReady) return
    document.documentElement.dataset.interface = resolveStyle(appearance.style, __SPACLENS_DESKTOP__, navigator.userAgent)
    document.documentElement.dataset.density = appearance.density
    window.localStorage.setItem(APPEARANCE_KEY, JSON.stringify(appearance))
  })
  $effect(() => {
    const scanId = workbench.activeScanId
    const size = minimumSize
    if (!scanId || view === 'browse' || workbench.status?.state !== 'ready' || !discoveryAvailable) return
    void size
    void refreshDiscovery(true)
  })
  let navigating = $state(false)
  let navigationRequest = 0
  let stream: { close: () => void } | null = null
  let pollTimer: ReturnType<typeof setInterval> | null = null

  /**
   * Where the window chrome sits, so the header clears it on the right side.
   *
   * macOS draws the traffic lights top-left (about 78px wide); Windows puts the
   * caption buttons top-right (three at roughly 46px each). In the browser
   * neither exists. Read from the user agent rather than a Tauri OS plugin, so
   * the desktop flavor needs no extra plugin for one padding decision.
   */
  const onWindows = typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent)
  function headerInset(): string {
    if (!__SPACLENS_DESKTOP__) return 'px-4'
    return onWindows ? 'pr-[152px] pl-4' : 'pl-[88px] pr-4'
  }

  const hosted = $derived(workbench.resolvedUrl !== null && !workbench.sameOrigin && !/^https?:\/\/(localhost|127\.0\.0\.1|\[::1\])(:|\/|$)/i.test(workbench.resolvedUrl) && workbench.phase !== 'ready')
  const ancestors = $derived(workbench.slice === null ? [] : [...workbench.slice.ancestors, workbench.slice.focusNode].map(node => ({ ...node, name: formatNodeName(node, isMacScan) })))
  const collectedIds = $derived(new Set(workbench.collector.map((entry) => entry.nodeId)))
  const collectorTotal = $derived(workbench.collector.reduce((total, entry) => total + entry.size, 0))
  const targets = $derived.by<ScanTarget[]>(() => {
    const current = workbench.targets ?? []
    const recentList = recentTargets
    const recent = recentList.map((entry, index) => ({
      id: `recent_${index}`,
      label: entry.label,
      path: entry.path,
      kind: 'folder' as const,
      description: '',
      size: 0,
      source: 'recent' as const,
      removable: true,
      lastScannedAt: entry.lastScannedAt,
    }))
    return [...current, ...recent].map(target => target.path === '/' && isMacScan ? { ...target, label: 'Macintosh HD' } : target)
  })

  /**
   * `~` means the user's home. On the desktop the home is known here; in the
   * browser it is not, so the path goes up unchanged and the server — which
   * runs on the same machine the paths name — expands it.
   */
  let desktopHome: string | null = null
  function expandTilde(path: string): string {
    const trimmed = path.trim()
    if (desktopHome === null) return trimmed
    if (trimmed === '~') return desktopHome
    if (trimmed.startsWith('~/') || trimmed.startsWith('~\\')) return `${desktopHome}${trimmed.slice(1)}`
    return trimmed
  }

  function stopStreams(): void {
    stream?.close()
    stream = null
    if (pollTimer !== null) clearInterval(pollTimer)
    pollTimer = null
  }

  async function connectDesktop(): Promise<void> {
    workbench.phase = 'connecting'
    try {
      const [{ createTauriService }, { loadTauriPorts }] = await Promise.all([
        import('@space-lens/client'),
        import('../lib/tauri-ports'),
      ])
      const ports = await loadTauriPorts()
      desktopHome = (await ports.homeDir?.().catch(() => null)) ?? null
      const service = createTauriService(ports)
      workbench.capabilities = await service.capabilities()
      const rootsResponse = await service.roots()
      workbench.targets = Array.isArray(rootsResponse?.roots) ? rootsResponse.roots : []
      workbench.service = service
      workbench.phase = 'ready'
      // Live push first; if the stream cannot start, the 2s polling below
      // stays as the fallback for the whole session.
      try {
        stream = await startDesktopEventStream(ports, {
          activeScanId: () => workbench.activeScanId,
          onStatus: (status) => (workbench.status = status),
          onReady: () => void loadRoot(),
        })
      } catch {
        stream = null
      }
      startPolling()
      const scans = await service.listScans?.().catch(() => [])
      const previous = scans?.find(scan => scan.state === 'scanning') ?? scans?.filter(scan => scan.state === 'ready').sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))[0]
      if (previous) {
        workbench.activeScanId = previous.scanId
        workbench.status = previous
        pickerOpen = false
        if (previous.state === 'ready') {
          await loadRoot()
          scannedPaths = await Promise.all(previous.rootIds.map(async nodeId => (await service.treeSlice({ scanId: previous.scanId, nodeId, depth: 0, maxChildrenPerNode: 1 })).focusNode.path))
        }
      }
    } catch (error) {
      workbench.phase = 'failed'
      workbench.connectMessage = `desktop IPC failed: ${describeError(error)}`
    }
  }

  async function connect(): Promise<void> {
    workbench.phase = 'connecting'
    workbench.connectMessage = null
    const resolved = resolveBaseUrl(explicitUrlInput || null)
    workbench.resolvedUrl = resolved.url
    workbench.sameOrigin = resolved.sameOrigin
    const [{ createHttpService }] = await Promise.all([import('@space-lens/client')])
    const service = createHttpService({ baseUrl: resolved.url, getToken: loadToken })
    try {
      const health = await service.health()
      if (health.apiMajor !== 1)
        throw new ServiceError(
          { code: 'UnsupportedOperation', message: `incompatible contract major ${health.apiMajor}`, retryable: false },
          400,
        )
      const ticket = ticketInput === '' ? ticketFromLocation() : ticketInput
      if (ticket || loadToken() === null) {
        const session = await service.exchange(ticket ?? '', hosted && passwordInput !== '' ? passwordInput : undefined)
        saveToken(session.token)
      }
      if (ticket !== null) {
        rememberBaseUrl(resolved.url)
        stripTicketFromLocation()
      }
      workbench.service = service
      workbench.capabilities = await service.capabilities()
      const rootsResponse = await service.roots()
      workbench.targets = Array.isArray(rootsResponse?.roots) ? rootsResponse.roots : []
      workbench.phase = 'ready'
      stream = startEventStream() ?? null
      startPolling()
      if (!new URLSearchParams(window.location.search).has('autoscan')) await restoreActiveScan()
    } catch (error) {
      clearToken()
      workbench.phase = 'failed'
      workbench.connectMessage = describeError(error)
    }
  }

  function startPolling(): void {
    if (pollTimer !== null) return
    pollTimer = setInterval(() => {
      const { service, status } = workbench
      if (service === null || status === null || status.state !== 'scanning') return
      void service
        .scanStatus(status.scanId)
        .then((fresh) => {
          if (workbench.activeScanId !== fresh.scanId) return
          workbench.status = fresh
          if (fresh.state === 'ready') void loadRoot()
        })
        .catch(() => {})
    }, 2_000)
  }

  async function restoreActiveScan(): Promise<void> {
    const previous = parseActiveScan(window.sessionStorage.getItem(ACTIVE_SCAN_KEY))
    const service = workbench.service
    if (!previous || !service) return
    try {
      const status = await service.scanStatus(previous.scanId)
      if (status.state !== 'scanning' && status.state !== 'ready') {
        window.sessionStorage.removeItem(ACTIVE_SCAN_KEY)
        return
      }
      scannedPaths = previous.paths
      workbench.activeScanId = status.scanId
      workbench.status = status
      pickerOpen = false
      if (status.state === 'ready') {
        if (previous.focusNodeId) await focusById(previous.focusNodeId)
        if (!workbench.slice) await loadRoot()
      }
    } catch {
      // An expired scan should not invalidate a working authenticated connection.
      window.sessionStorage.removeItem(ACTIVE_SCAN_KEY)
    }
  }

  async function startScan(paths: string[]): Promise<void> {
    const { service } = workbench
    if (service === null || startingScan || workbench.deleting) return
    startingScan = true
    workbench.error = null
    try {
      const session = await service.startScan({
        localOnly: true,
        paths: paths.map(expandTilde),
        ignoreHidden: false,
        respectGitignore: !paths.some(path => path === '/' || path === '/System/Volumes/Data'),
        ignoredMode: 'summarize',
        label: paths.includes('/') && paths.includes('/System/Volumes/Data') ? 'Macintosh HD' : paths[0],
      })
      for (const path of paths) rememberRecentTarget(path)
      recentTargets = loadRecentTargets()
      scannedPaths = paths.map(expandTilde)
      pickerOpen = false
      view = 'browse'
      browseSearch = ''
      browseSort = 'size'
      notice = null
      cleanupPlan = null
      cleanupError = null
      discoverySequence++
      navigationRequest++
      navigating = false
      discoveryLoading = false
      discoveryItems = []
      discoveryPage = null
      workbench.activeScanId = session.scanId
      if (!__SPACLENS_DESKTOP__) window.sessionStorage.setItem(ACTIVE_SCAN_KEY, JSON.stringify({ scanId: session.scanId, paths: scannedPaths }))
      workbench.status = {
        scanId: session.scanId,
        state: 'scanning',
        message: '',
        progress: null,
        currentPath: null,
        bytesScanned: 0,
        entriesScanned: 0,
        rootIds: [],
        label: session.label,
        updatedAt: session.createdAt,
      }
      workbench.slice = null
      workbench.items = []
      workbench.collector = []
      // The desktop worker answers the session while still scanning; the
      // event stream (polling fallback) drives the status to ready. This
      // immediate status call is best-effort — the loadRoot guard ignores it
      // until the scan actually reaches ready.
      if (__SPACLENS_DESKTOP__ && workbench.service !== null) {
        try {
          workbench.status = await workbench.service.scanStatus(session.scanId)
        } catch {
          // keep the optimistic scanning status; the stream will refresh it
        }
        await loadRoot()
      }
    } catch (error) {
      workbench.error = describeError(error)
    } finally { startingScan = false }
  }

  async function cancelScan(): Promise<void> {
    const { service, status } = workbench
    if (service === null || status === null) return
    try { workbench.status = await service.cancelScan(status.scanId) } catch (error) { workbench.error = describeError(error) }
  }

  async function focus(node: TreeNodeSummary): Promise<void> {
    if (node.scanState === 'skipped') { notice = `Not scanned: ${node.skipReason ?? 'unavailable'}. See scan details for coverage.`; return }
    const { service, status } = workbench
    if (service === null || status === null || status.state !== 'ready') return
    const request = ++navigationRequest
    navigating = true
    workbench.error = null
    try {
      const [slice, page] = await Promise.all([
        service.treeSlice({ scanId: status.scanId, nodeId: node.id, depth: 3, maxChildrenPerNode: 50 }),
        service.children({ scanId: status.scanId, nodeId: node.id, offset: 0, limit: 200, sort: browseSort }),
      ])
      if (request !== navigationRequest || workbench.activeScanId !== status.scanId) return
      workbench.hoveredId = null
      workbench.slice = slice
      workbench.items = page.items
      workbench.childrenTotal = page.total
      browseSearch = ''
      if (!__SPACLENS_DESKTOP__) window.sessionStorage.setItem(ACTIVE_SCAN_KEY, JSON.stringify({ scanId: status.scanId, paths: scannedPaths, focusNodeId: node.id }))
    } catch (error) {
      if (request === navigationRequest) workbench.error = describeError(error)
    } finally {
      if (request === navigationRequest) navigating = false
    }
  }

  function goUp(): void {
    const parent = workbench.slice?.ancestors.at(-1)
    if (parent) void focus(parent)
  }

  function invalidatePlan(): void { cleanupPlan = null; cleanupError = null }
  function collect(node: TreeNodeSummary): void {
    if (workbench.status === null || workbench.deleting || planning) return
    invalidatePlan()
    workbench.collector = addSelection(workbench.collector, node, workbench.status.scanId, new Date().toISOString(), `col_${crypto.randomUUID().slice(0, 8)}`)
  }
  function uncollect(node: TreeNodeSummary): void {
    if (workbench.deleting || planning) return
    invalidatePlan()
    workbench.collector = workbench.collector.filter(entry => entry.nodeId !== node.id)
  }
  function toggleCollected(node: TreeNodeSummary): void {
    if (collectedIds.has(node.id)) uncollect(node)
    else collect(node)
  }
  function removeEntry(id: string): void {
    invalidatePlan()
    workbench.collector = workbench.collector.filter(entry => entry.id !== id)
  }
  function changeView(next: 'browse' | DiscoveryKind): void {
    navigationRequest++
    navigating = false
    view = next
    if (next !== 'browse') minimumSize = next === 'large-files' ? 10 * 1024 ** 2 : 0
    if (window.innerWidth < 760) sidebarVisible = false
  }
  async function refreshDiscovery(reset: boolean): Promise<void> {
    const { service, status } = workbench
    if (!service?.discover || status?.state !== 'ready' || view === 'browse') return
    const request = ++discoverySequence
    const kind = view
    const offset = reset ? 0 : discoveryItems.length
    discoveryLoading = true
    discoveryError = null
    if (reset) { discoveryPage = null; discoveryItems = [] }
    try {
      const page = await service.discover({ scanId: status.scanId, kind, minSize: minimumSize, offset, limit: 200 })
      if (request !== discoverySequence || status.scanId !== workbench.activeScanId || view !== kind) return
      discoveryPage = page
      discoveryItems = reset ? page.items : [...discoveryItems, ...page.items]
    } catch (error) {
      if (request === discoverySequence) discoveryError = describeError(error)
    } finally {
      if (request === discoverySequence) discoveryLoading = false
    }
  }
  async function loadMoreChildren(): Promise<void> {
    const { service, status, slice } = workbench
    if (!service || !status || !slice || navigating) return
    const request = ++navigationRequest
    navigating = true
    try {
      const page = await service.children({scanId: status.scanId, nodeId: slice.focusNode.id, offset: workbench.items.length, limit: 200, sort: browseSort})
      if (request !== navigationRequest || status.scanId !== workbench.activeScanId) return
      workbench.items = [...workbench.items, ...page.items]
      workbench.childrenTotal = page.total
    } catch (error) { workbench.error = describeError(error) }
    finally { if (request === navigationRequest) navigating = false }
  }
  async function focusById(nodeId: string, showBrowse = false): Promise<void> {
    const { service, status } = workbench
    if (!service || !status) return
    const request = ++navigationRequest
    const sourceView = view
    try {
      const slice = await service.treeSlice({scanId: status.scanId, nodeId, depth: 3, maxChildrenPerNode: 50})
      if (request !== navigationRequest || status.scanId !== workbench.activeScanId || view !== sourceView) return
      if (showBrowse) changeView('browse')
      await focus(slice.focusNode)
    } catch (error) {
      if (request === navigationRequest && status.scanId === workbench.activeScanId) workbench.error = describeError(error)
    }
  }
  async function browseDiscovered(item: DiscoveryItem): Promise<void> {
    const nodeId = item.parentId ?? workbench.status?.rootIds[0]
    if (nodeId) await focusById(nodeId, true)
  }
  async function copyPath(path: string): Promise<void> {
    try { await navigator.clipboard.writeText(path); notice = 'Path copied' }
    catch { notice = `Copy this path: ${path}` }
  }
  function newLocation(path = ''): void {
    if (workbench.deleting || planning) return
    navigationRequest++
    navigating = false
    pickerPath = path
    pickerOpen = true
  }
  function forgetRecent(path: string): void {
    recentTargets = recentTargets.filter(entry => entry.path !== path)
    window.localStorage.setItem('spacelens.recentScans', JSON.stringify(recentTargets))
  }
  async function reviewCleanup(): Promise<void> {
    const { service, status, collector } = workbench
    if (!service || !status || !collector.length || !workbench.capabilities?.cleanup.plan || planning) return
    planning = true
    cleanupError = null
    cleanupOutcome = null
    try { cleanupPlan = await service.plan({ scanId: status.scanId, nodeIds: collector.map(entry => entry.nodeId) }) }
    catch (error) { cleanupError = describeError(error) }
    finally { planning = false }
  }
  async function confirmCleanup(): Promise<void> {
    const { service } = workbench
    const plan = cleanupPlan
    if (!service || !plan || workbench.deleting || !workbench.capabilities?.cleanup.execute) return
    workbench.deleting = true
    cleanupError = null
    try {
      cleanupOutcome = await service.execute({ planId: plan.planId, confirm: true })
      const trashedPaths = new Set(cleanupOutcome.trashed.map(entry => entry.path))
      workbench.collector = workbench.collector.filter(entry => !trashedPaths.has(entry.path))
      cleanupPlan = null
      notice = `${cleanupOutcome.trashed.length} items moved to Trash. Rescan to update the storage map.`
    } catch (error) { cleanupError = describeError(error); cleanupPlan = null }
    finally { workbench.deleting = false }
  }

  onMount(() => {
    appearance = parseAppearance(window.localStorage.getItem(APPEARANCE_KEY))
    preferencesReady = true
    if (window.innerWidth < 760) sidebarVisible = false
    if (__SPACLENS_DESKTOP__) {
      void connectDesktop()
      return () => stopStreams()
    }
    const resolved = resolveBaseUrl(null)
    workbench.resolvedUrl = resolved.url
    workbench.sameOrigin = resolved.sameOrigin
    // A ticket in the URL pairs automatically — the terminal-printed pairing
    // URL is meant to land the user on a working session in one step.
    const initialTicket = ticketFromLocation()
    const autoscan = new URLSearchParams(window.location.search).get('autoscan')
    if (initialTicket !== null || loadToken() !== null) {
      void connect().then(() => {
        if (autoscan === null || workbench.phase !== 'ready') return
        return startScan(workbench.targets.map((target) => target.path))
      })
    }
    return () => stopStreams()
  })
</script>

{#if workbench.phase !== 'ready'}
  <div data-tauri-drag-region class="fixed top-0 right-0 left-0 z-40 h-10"></div>
  <ConnectionPanel
    phase={workbench.phase === 'failed' ? 'failed' : workbench.phase === 'connecting' ? 'connecting' : 'idle'}
    resolvedUrl={workbench.resolvedUrl} sameOrigin={workbench.sameOrigin} {hosted}
    ticket={ticketInput} password={passwordInput} message={workbench.connectMessage}
    onBaseUrl={url => (explicitUrlInput = url)} onTicket={value => (ticketInput = value)} onPassword={value => (passwordInput = value)} onConnect={() => void connect()}
  />
{:else if pickerOpen || workbench.status === null || workbench.status.state !== 'ready'}
  <ScanPicker {targets} mode={__SPACLENS_DESKTOP__ ? 'desktop' : 'browser'}
    folderPicker={workbench.capabilities?.host.folderPicker ?? false}
    onSettings={() => (settingsOpen = true)}
    onPickFolder={() => workbench.service?.pickFolder?.() ?? Promise.resolve(null)}
    chromeInset={headerInset()} logo={`${base}/logo-mark.png`} initialPath={pickerPath}
    canClose={workbench.status?.state === 'ready'} busy={startingScan || workbench.status?.state === 'scanning'}
    error={workbench.error || (workbench.status?.state === 'failed' ? workbench.status.message : workbench.status?.state === 'cancelled' ? 'Scan cancelled. Choose a location to start again.' : null)}
    status={workbench.status} onScan={paths => void startScan(paths)} onForget={forgetRecent}
    onCancel={() => { if (workbench.status?.state === 'scanning') void cancelScan(); else pickerOpen = false }}
  />
{:else}
  <div class="workbench-shell compact-workbench">
    <LensToolbar title={displayFocus?.name ?? workbench.status.label ?? 'Storage'}
      mode={__SPACLENS_DESKTOP__ ? 'desktop' : 'browser'} chromeInset={headerInset()} {sidebarVisible} {chartVisible}
      collectorCount={workbench.collector.length} busy={workbench.deleting || planning}
      onNewScan={() => newLocation()} onToggleSidebar={() => (sidebarVisible = !sidebarVisible)}
      onToggleChart={() => { chartVisible = !chartVisible; view = 'browse' }}
      onOpenCollector={() => (collectorOpen = true)} onOpenSettings={() => (settingsOpen = true)}
    />
    <div class="workspace-body">
      {#if sidebarVisible}
        <ScanSidebar {targets} {view} {discoveryAvailable} {gitignoredAvailable} selectedPath={ancestors[0]?.path ?? scannedPaths[0] ?? workbench.status.label}
          busy={workbench.deleting || planning} onView={changeView} onSelect={target => newLocation(target.path)} onCustom={() => newLocation()} onForget={forgetRecent} />
      {/if}
      <div class="workspace-content">
        {#if workbench.error}<StateBanner state="error" title="Could not complete the request" detail={workbench.error} />{/if}
        {#if notice}<div class="workspace-notice" role="status"><span>{notice}</span>{#if cleanupOutcome}<button type="button" disabled={workbench.deleting} onclick={() => { cleanupOutcome = null; void startScan(scannedPaths.length ? scannedPaths : [workbench.status?.label ?? '']) }}>Rescan</button>{/if}<button type="button" aria-label="Dismiss notice" onclick={() => (notice = null)}>×</button></div>{/if}
        <ScanCoveragePanel status={workbench.status} />
        {#if view === 'browse'}
          <div class="explorer-pathbar">
            {#if workbench.status.rootIds.length > 1}<LensSelect class="root-select" label="Scanned location" value={ancestors[0]?.id ?? workbench.status.rootIds[0]} options={scannedLocationOptions} onChange={value => void focusById(value)} />{/if}
            <BreadcrumbBar items={ancestors} onSelect={node => void focus(node)} onBack={goUp} canGoBack={ancestors.length > 1} />
            <span>{navigating ? 'Opening…' : formatBytes(workbench.slice?.totalSize ?? workbench.status.bytesScanned)}</span>
          </div>
          <main class="explorer-layout" class:without-chart={!chartVisible} aria-busy={navigating}>
            {#if chartVisible}<div class="explorer-chart">
              <SunburstChart tree={workbench.slice?.tree ?? null} focusNode={displayFocus}
                hoveredNode={workbench.items.find(item => item.id === workbench.hoveredId) ?? null} onBack={goUp} canGoBack={ancestors.length > 1}
                hoveredId={workbench.hoveredId} collectedIds={coveredIds} onHover={id => (workbench.hoveredId = id)}
                onOpen={node => void focus(node)} onContext={toggleCollected} />
              {#if workbench.slice?.truncated}<p class="chart-summary">{workbench.slice.omittedCount.toLocaleString()} smaller items grouped · open a folder to explore</p>{/if}
            </div>{/if}
            <aside class="explorer-sidebar" aria-label="Folder contents">
              <div class="contents-heading"><div><h2>Folder contents</h2><p>{workbench.childrenTotal.toLocaleString()} item{workbench.childrenTotal === 1 ? '' : 's'} · {workbench.items.length.toLocaleString()} loaded</p></div><div class="browse-sort"><LensSelect value={browseSort} options={sortOptions} label="Sort folder contents" class="w-32" onChange={value => { browseSort = value === 'name' ? 'name' : value === 'path' ? 'path' : 'size'; if (workbench.slice) void focus(workbench.slice.focusNode) }} /></div></div>
              <label class="search-field browse-search"><Input.Root type="search" bind:value={browseSearch} aria-label="Search folder contents" placeholder="Search loaded items…" /></label>
              <div class="browse-selection"><button type="button" disabled={workbench.deleting || planning || !selectableShown.length} onclick={() => selectableShown.forEach(collect)}>Select shown ({selectableShown.length})</button>{#if workbench.collector.length}<button type="button" disabled={workbench.deleting || planning} onclick={() => { invalidatePlan(); workbench.collector = [] }}>Clear selection</button>{/if}</div>
              {#key workbench.slice?.focusNode.id}<ChildList disabled={planning || workbench.deleting} isCovered={node => { const entry = selectedAncestor(workbench.collector, node.path); return Boolean(entry && entry.nodeId !== node.id) }} totalSize={workbench.slice?.focusNode.size ?? 0} items={selectedItems} hoveredId={workbench.hoveredId} collectedIds={coveredIds}
                onHover={id => (workbench.hoveredId = id)} onOpen={node => void focus(node)} onCollect={collect} onRemove={uncollect} />{/key}
              {#if workbench.items.length < workbench.childrenTotal}<div class="load-more"><Button.Root variant="ghost" size="sm" disabled={navigating} onclick={() => void loadMoreChildren()}>Load more items</Button.Root></div>{/if}
            </aside>
          </main>
        {:else}
          {#key view}<DiscoveryList kind={view} page={discoveryPage} items={discoveryItems} loading={discoveryLoading} error={discoveryError} minSize={minimumSize}
            disabled={workbench.deleting || planning} isSelected={node => Boolean(selectedAncestor(workbench.collector, node.path))} isCovered={node => { const entry = selectedAncestor(workbench.collector, node.path); return Boolean(entry && entry.nodeId !== node.id) }}
            onToggle={toggleCollected} onSelect={nodes => nodes.forEach(collect)} onDeselect={nodes => nodes.forEach(uncollect)}
            onMinSize={value => (minimumSize = value)} onLoadMore={() => void refreshDiscovery(false)} onRetry={() => void refreshDiscovery(true)}
            onBrowse={item => void browseDiscovered(item)} onCopy={path => void copyPath(path)} />{/key}
        {/if}
      </div>
    </div>
    <footer class="workspace-status"><span><i class:reconnecting={!__SPACLENS_DESKTOP__ && workbench.streamState !== 'live'}></i>{formatBytes(workbench.status.bytesScanned)} scanned <span class="status-separator">·</span> {workbench.status.coverage ? (__SPACLENS_DESKTOP__ ? 'Local cleanup enabled' : 'Read-only local scan') : workbench.capabilities?.cleanup.execute ? 'Local cleanup enabled' : 'Read-only connection'}</span><button type="button" onclick={() => (collectorOpen = true)}><span>{workbench.collector.length} selected</span><strong>{formatBytes(collectorTotal)}</strong><span>Review →</span></button></footer>
    <CollectorPanel open={collectorOpen} entries={workbench.collector} totalSize={collectorTotal} deleting={workbench.deleting} {planning}
      plan={cleanupPlan} outcome={cleanupOutcome} error={cleanupError} {cleanupAvailable}
      onClose={() => { collectorOpen = false; cleanupPlan = null }} onRemove={removeEntry} onClear={() => { invalidatePlan(); workbench.collector = [] }} onDelete={() => void reviewCleanup()} onConfirm={() => void confirmCleanup()} />
  </div>
{/if}
<AppearancePanel open={settingsOpen} style={appearance.style} density={appearance.density} mode={userPrefersMode.current}
  automaticStyle={automaticStyle === 'macos' ? 'macOS on this platform' : automaticStyle === 'windows' ? 'Windows on this platform' : 'Web on this platform'}
  appVersion={appVersion ?? undefined} update={updateStatus}
  onClose={() => (settingsOpen = false)} onStyle={style => (appearance.style = style)} onDensity={density => (appearance.density = density)} onMode={setMode}
  onCheckUpdates={__SPACLENS_DESKTOP__ ? () => void checkForAppUpdate(status => (updateStatus = status)) : undefined}
  onInstallUpdate={__SPACLENS_DESKTOP__ ? () => void installAppUpdate(status => (updateStatus = status)) : undefined} />
