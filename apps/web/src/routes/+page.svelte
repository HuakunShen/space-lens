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
  } from '@space-lens/web-ui'
  import type { CollectorEntry, ScanTarget, TreeNodeSummary } from '@space-lens/web-ui/types'
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

  let ticketInput = $state('')
  let passwordInput = $state('')
  let explicitUrlInput = $state('')
  let collectorOpen = $state(false)
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

  const hosted = $derived(workbench.resolvedUrl !== null && !workbench.sameOrigin && workbench.phase !== 'ready')
  const ancestors = $derived(workbench.slice === null ? [] : [...workbench.slice.ancestors, workbench.slice.focusNode])
  const collectedIds = $derived(new Set(workbench.collector.map((entry) => entry.nodeId)))
  const collectorTotal = $derived(workbench.collector.reduce((total, entry) => total + entry.size, 0))
  const targets = $derived.by<ScanTarget[]>(() => {
    const current = workbench.targets ?? []
    const recentList = loadRecentTargets() ?? []
    const recent = recentList.map((entry, index) => ({
      id: `recent_${index}`,
      label: entry.label,
      path: entry.path,
      kind: 'folder' as const,
      description: '',
      size: 0,
      source: 'recent' as const,
      removable: false,
      lastScannedAt: entry.lastScannedAt,
    }))
    return [...current, ...recent]
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
      const session = await service.exchange(ticket ?? '', hosted && passwordInput !== '' ? passwordInput : undefined)
      saveToken(session.token)
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
          workbench.status = fresh
          if (fresh.state === 'ready') void loadRoot()
        })
        .catch(() => {})
    }, 2_000)
  }

  async function startScan(paths: string[]): Promise<void> {
    const { service } = workbench
    if (service === null) return
    workbench.error = null
    try {
      const session = await service.startScan({
        paths: paths.map(expandTilde),
        ignoreHidden: false,
        respectGitignore: true,
        ignoredMode: 'summarize',
        label: paths[0],
      })
      for (const path of paths) rememberRecentTarget(path)
      workbench.activeScanId = session.scanId
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
      // the desktop engine scans synchronously: the session answers ready
      if (__SPACLENS_DESKTOP__ && workbench.service !== null) {
        workbench.status = await workbench.service.scanStatus(session.scanId)
        await loadRoot()
      }
    } catch (error) {
      workbench.error = describeError(error)
    }
  }

  async function cancelScan(): Promise<void> {
    const { service, status } = workbench
    if (service === null || status === null) return
    await service.cancelScan(status.scanId).catch(() => {})
  }

  async function focus(node: TreeNodeSummary): Promise<void> {
    const { service, status } = workbench
    if (service === null || status === null || status.state !== 'ready') return
    const request = ++navigationRequest
    navigating = true
    workbench.error = null
    try {
      const [slice, page] = await Promise.all([
        service.treeSlice({ scanId: status.scanId, nodeId: node.id, depth: 3, maxChildrenPerNode: 50 }),
        service.children({ scanId: status.scanId, nodeId: node.id, offset: 0, limit: 200, sort: 'size' }),
      ])
      if (request !== navigationRequest) return
      workbench.hoveredId = null
      workbench.slice = slice
      workbench.items = page.items
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

  function collect(node: TreeNodeSummary): void {
    const { status } = workbench
    if (status === null) return
    if (workbench.collector.some((entry) => entry.nodeId === node.id)) return
    // an ancestor supersedes its staged descendants
    workbench.collector = workbench.collector.filter((entry) => !entry.path.startsWith(`${node.path}/`))
    const entry: CollectorEntry = {
      id: `col_${crypto.randomUUID().slice(0, 8)}`,
      scanId: status.scanId,
      nodeId: node.id,
      path: node.path,
      name: node.name,
      size: node.size,
      addedAt: new Date().toISOString(),
    }
    workbench.collector = [...workbench.collector, entry]
  }

  function uncollect(node: TreeNodeSummary): void {
    workbench.collector = workbench.collector.filter((entry) => entry.nodeId !== node.id)
  }

  /** Right-click on the chart toggles the node's collector membership. */
  function toggleCollected(node: TreeNodeSummary): void {
    if (collectedIds.has(node.id)) uncollect(node)
    else collect(node)
  }

  async function deleteCollected(): Promise<void> {
    const { service, status, collector } = workbench
    if (service === null || status === null || collector.length === 0) return
    const allowCleanup = workbench.capabilities?.cleanup.execute ?? false
    if (!allowCleanup) {
      workbench.error = 'this server was started without --allow-cleanup'
      return
    }
    if (!window.confirm(`Move ${collector.length} item(s) to the trash? Nothing is permanently deleted.`)) return
    workbench.deleting = true
    workbench.error = null
    try {
      const plan = await service.plan({ scanId: status.scanId, nodeIds: collector.map((entry) => entry.nodeId) })
      const outcome = await service.execute({ planId: plan.planId, confirm: true })
      const trashedPaths = new Set(outcome.trashed.map((entry) => entry.path))
      workbench.collector = collector.filter((entry) => !trashedPaths.has(entry.path))
      if (outcome.failed.length > 0) {
        workbench.error = `${outcome.failed.length} item(s) could not be trashed: ${outcome.failed[0]!.message}`
      }
      await loadRoot()
    } catch (error) {
      workbench.error = describeError(error)
    } finally {
      workbench.deleting = false
    }
  }

  onMount(() => {
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
    if (initialTicket !== null) {
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
    resolvedUrl={workbench.resolvedUrl}
    sameOrigin={workbench.sameOrigin}
    {hosted}
    ticket={ticketInput}
    password={passwordInput}
    message={workbench.connectMessage}
    onBaseUrl={(url) => (explicitUrlInput = url)}
    onTicket={(value) => (ticketInput = value)}
    onPassword={(value) => (passwordInput = value)}
    onConnect={() => void connect()}
  />
{:else}
  <div class="workbench-shell flex min-h-screen flex-col">
    {#if workbench.status === null || workbench.status.state === 'idle'}
      <ScanPicker
        {targets}
        mode={__SPACLENS_DESKTOP__ ? 'desktop' : 'browser'}
        folderPicker={workbench.capabilities?.host.folderPicker ?? false}
        onPickFolder={() => workbench.service?.pickFolder?.() ?? Promise.resolve(null)}
        chromeInset={headerInset()}
        logo={`${base}/logo-mark.png`}
        busy={false}
        error={workbench.error}
        status={null}
        onScan={(paths) => void startScan(paths)}
        onCancel={() => {}}
      />
    {:else if workbench.status.state === 'scanning'}
      <ScanPicker
        {targets}
        mode={__SPACLENS_DESKTOP__ ? 'desktop' : 'browser'}
        folderPicker={workbench.capabilities?.host.folderPicker ?? false}
        onPickFolder={() => workbench.service?.pickFolder?.() ?? Promise.resolve(null)}
        chromeInset={headerInset()}
        logo={`${base}/logo-mark.png`}
        busy={true}
        error={workbench.error}
        status={workbench.status}
        onScan={() => {}}
        onCancel={() => void cancelScan()}
      />
      <StateBanner
        state="loading"
        title="Scanning {workbench.status.label ?? '…'}"
        detail="The engine reports no progress; this finishes when the tree is complete."
      />
    {:else if workbench.status.state === 'ready'}
      <header
        data-tauri-drag-region
        class={[
          'flex h-12 shrink-0 items-center justify-between border-b',
          headerInset(),
        ]}
      >
        <div class="flex items-center gap-2.5">
          <img src={`${base}/logo-mark.png`} alt="" class="size-8 shrink-0 rounded-lg" />
          <span class="font-semibold">Space Lens</span>
        </div>
        <span class="rounded-full border px-2 py-0.5 text-xs">{__SPACLENS_DESKTOP__ ? 'desktop' : 'browser'}</span>
      </header>
      {#if workbench.error}
        <StateBanner state="error" title="Something failed" detail={workbench.error} />
      {/if}

      {#if !__SPACLENS_DESKTOP__ && workbench.streamState !== 'live'}
        <StateBanner
          state="disconnected"
          title="no live updates ({workbench.streamState})"
          detail="Data still loads on demand."
        />
      {/if}
      <div class="explorer-pathbar">
        <BreadcrumbBar
          items={ancestors}
          onSelect={(node) => void focus(node)}
          onBack={goUp}
          canGoBack={ancestors.length > 1}
        />
        <span role="status" class="text-xs text-muted-foreground"
          >{navigating ? 'Opening folder…' : 'Click a folder to explore'}</span
        >
      </div>
      <main class="explorer-layout" aria-busy={navigating}>
        <div class="explorer-chart">
          <SunburstChart
            tree={workbench.slice?.tree ?? null}
            focusNode={workbench.slice?.focusNode ?? null}
            hoveredNode={workbench.items.find((item) => item.id === workbench.hoveredId) ?? null}
            onBack={goUp}
            canGoBack={ancestors.length > 1}
            hoveredId={workbench.hoveredId}
            {collectedIds}
            onHover={(id) => (workbench.hoveredId = id)}
            onOpen={(node) => void focus(node)}
            onContext={(node) => toggleCollected(node)}
          />
          {#if workbench.slice?.truncated}
            <p class="chart-summary">
              {workbench.slice.omittedCount.toLocaleString()} smaller items grouped · open a folder for more detail
            </p>
          {/if}
        </div>
        <aside class="explorer-sidebar" aria-label="Folder contents">
          <div class="contents-heading">
            <div>
              <h2>Folder contents</h2>
              <p>{workbench.items.length.toLocaleString()} shown · largest first</p>
            </div>
            <span>SIZE</span>
          </div>
          <ChildList
            totalSize={workbench.slice?.focusNode.size ?? 0}
            items={workbench.items}
            hoveredId={workbench.hoveredId}
            {collectedIds}
            onHover={(id) => (workbench.hoveredId = id)}
            onOpen={(node) => void focus(node)}
            onCollect={(node) => collect(node)}
            onRemove={(node) => uncollect(node)}
          />
        </aside>
      </main>
      <StatusBar
        status={workbench.status}
        {collectorTotal}
        collectorCount={workbench.collector.length}
        onOpenCollector={() => (collectorOpen = true)}
        onCancel={() => void cancelScan()}
      />
      <CollectorPanel
        open={collectorOpen}
        entries={workbench.collector}
        totalSize={collectorTotal}
        deleting={workbench.deleting}
        onClose={() => (collectorOpen = false)}
        onRemove={(id) => (workbench.collector = workbench.collector.filter((entry) => entry.id !== id))}
        onDelete={() => void deleteCollected()}
      />
    {:else if workbench.status.state === 'failed'}
      <StateBanner
        state="error"
        title="Scan failed"
        detail={workbench.status.message}
        action={{ label: 'Start over', onClick: () => (workbench.status = null) }}
      />
    {:else if workbench.status.state === 'cancelled'}
      <StateBanner
        state="info"
        title="Scan cancelled"
        action={{ label: 'Start over', onClick: () => (workbench.status = null) }}
      />
    {/if}
  </div>
{/if}
