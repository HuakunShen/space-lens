<script lang="ts">
  import { onMount } from 'svelte'
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
  let stream: { close: () => void } | null = null
  let pollTimer: ReturnType<typeof setInterval> | null = null

  const hosted = $derived(
    workbench.resolvedUrl !== null && !workbench.sameOrigin && workbench.phase !== 'ready',
  )
  const ancestors = $derived(workbench.slice === null ? [] : [...workbench.slice.ancestors, workbench.slice.focusNode])
  const collectedIds = $derived(new Set(workbench.collector.map((entry) => entry.nodeId)))
  const collectorTotal = $derived(workbench.collector.reduce((total, entry) => total + entry.size, 0))
  const targets = $derived.by<ScanTarget[]>(() => {
    const recent = loadRecentTargets().map((entry, index) => ({
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
    return [...workbench.targets, ...recent]
  })

  function stopStreams(): void {
    stream?.close()
    stream = null
    if (pollTimer !== null) clearInterval(pollTimer)
    pollTimer = null
  }

  async function connectDesktop(): Promise<void> {
    workbench.phase = 'connecting'
    try {
      const [{ createTauriService }] = await Promise.all([import('../lib/tauri-service')])
      const service = createTauriService()
      workbench.capabilities = await service.capabilities()
      workbench.targets = (await service.roots()).roots
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
      if (health.apiMajor !== 1) throw new ServiceError({ code: 'UnsupportedOperation', message: `incompatible contract major ${health.apiMajor}`, retryable: false }, 400)
      const ticket = ticketInput === '' ? ticketFromLocation() : ticketInput
      const session = await service.exchange(ticket ?? '', hosted && passwordInput !== '' ? passwordInput : undefined)
      saveToken(session.token)
      if (ticket !== null) {
        rememberBaseUrl(resolved.url)
        stripTicketFromLocation()
      }
      workbench.service = service
      workbench.capabilities = await service.capabilities()
      workbench.targets = (await service.roots()).roots
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
        paths,
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
    try {
      workbench.slice = await service.treeSlice({ scanId: status.scanId, nodeId: node.id, depth: 3, maxChildrenPerNode: 50 })
      const page = await service.children({ scanId: status.scanId, nodeId: node.id, offset: 0, limit: 200, sort: 'size' })
      workbench.items = page.items
    } catch (error) {
      workbench.error = describeError(error)
    }
  }

  function collect(node: TreeNodeSummary): void {
    const { status } = workbench
    if (status === null) return
    if (workbench.collector.some((entry) => entry.nodeId === node.id)) return
    // an ancestor supersedes its staged descendants
    workbench.collector = workbench.collector.filter(
      (entry) => !entry.path.startsWith(`${node.path}/`),
    )
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
  <ConnectionPanel
    phase={workbench.phase === 'failed' ? 'failed' : workbench.phase === 'connecting' ? 'connecting' : 'idle'}
    resolvedUrl={workbench.resolvedUrl}
    sameOrigin={workbench.sameOrigin}
    hosted={hosted}
    ticket={ticketInput}
    password={passwordInput}
    message={workbench.connectMessage}
    onBaseUrl={(url) => (explicitUrlInput = url)}
    onTicket={(value) => (ticketInput = value)}
    onPassword={(value) => (passwordInput = value)}
    onConnect={() => void connect()}
  />
{:else}
  <div class="flex min-h-screen flex-col gap-3 p-4">
    {#if workbench.status === null || workbench.status.state === 'idle'}
      <ScanPicker
        {targets}
        mode="browser"
        busy={false}
        error={workbench.error}
        status={null}
        onScan={(paths) => void startScan(paths)}
        onCancel={() => {}}
      />
    {:else if workbench.status.state === 'scanning'}
      <ScanPicker
        {targets}
        mode="browser"
        busy={true}
        error={workbench.error}
        status={workbench.status}
        onScan={() => {}}
        onCancel={() => void cancelScan()}
      />
      <StateBanner state="loading" title="Scanning {workbench.status.label ?? '…'}" detail="The engine reports no progress; this finishes when the tree is complete." />
    {:else if workbench.status.state === 'ready'}
      {#if workbench.error}
        <StateBanner state="error" title="Something failed" detail={workbench.error} />
      {/if}
      {#if workbench.slice?.truncated}
        <StateBanner state="truncated" title="Part of this view is collapsed" detail="{workbench.slice.omittedCount} children were summarized; open a folder to go deeper." />
      {/if}
      {#if workbench.streamState !== 'live'}
        <StateBanner state="disconnected" title="no live updates ({workbench.streamState})" detail="Data still loads on demand." />
      {/if}
      <div class="grid flex-1 grid-cols-1 gap-4 lg:grid-cols-[minmax(0,1fr)_360px]">
        <div class="flex min-w-0 flex-col items-center gap-3">
          <SunburstChart
            tree={workbench.slice?.tree ?? null}
            focusNode={workbench.slice?.focusNode ?? null}
            hoveredId={workbench.hoveredId}
            collectedIds={collectedIds}
            onHover={(id) => (workbench.hoveredId = id)}
            onOpen={(node) => void focus(node)}
            onContext={(node, x, y) => collect(node)}
          />
          <BreadcrumbBar items={ancestors} onSelect={(node) => void focus(node)} />
        </div>
        <ChildList
          items={workbench.items}
          hoveredId={workbench.hoveredId}
          collectedIds={collectedIds}
          onHover={(id) => (workbench.hoveredId = id)}
          onOpen={(node) => void focus(node)}
          onCollect={(node) => collect(node)}
          onContext={(node) => collect(node)}
        />
      </div>
      <StatusBar
        status={workbench.status}
        collectorTotal={collectorTotal}
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
      <StateBanner state="error" title="Scan failed" detail={workbench.status.message} action={{ label: 'Start over', onClick: () => (workbench.status = null) }} />
    {:else if workbench.status.state === 'cancelled'}
      <StateBanner state="info" title="Scan cancelled" action={{ label: 'Start over', onClick: () => (workbench.status = null) }} />
    {/if}
  </div>
{/if}
