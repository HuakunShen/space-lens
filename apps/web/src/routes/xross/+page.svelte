<script lang="ts">
  import { onMount } from 'svelte'
  import { ScanPicker, StateBanner } from '@space-lens/web-ui'
  import type { CollectorEntry, ScanTarget, TreeNodeSummary, TreeSlice, ScanStatus } from '@space-lens/contract'
  import { connectXrossView, readyRootNodeId, xrossHostFromWindow, XrossViewError, type XrossView } from '@space-lens/client/xross-view'
  import { projectNode, projectScanStatus, projectTreeSlice } from '@space-lens/client/xross-projection'
  import type { RootSummaryV1, ScanStatusV1, CleanupPlanV1 } from '../../../../../integrations/xross/view-contract/surfaces/space-lens/api.js'
  import type { SpaceNodeIdV1, SpaceRootIdV1 } from '../../../../../integrations/xross/view-contract/contracts/view-v1/ids.js'
  import { parseSpaceLensId } from '../../../../../integrations/xross/view-contract/contracts/view-v1/ids.js'
  import { provideLensI18n } from '@space-lens/web-ui/i18n'
  import WorkbenchSurface from '../../lib/workbench/WorkbenchSurface.svelte'
  import { createICloudEviction } from '../../lib/xross-icloud-eviction.svelte'

  let view = $state<XrossView | null>(null)
  let roots = $state<RootSummaryV1[]>([])
  let rootsCursor = $state<string | null>(null)
  let loadingRoots = $state(false)
  let scan = $state<ScanStatusV1 | null>(null)
  let slice = $state<TreeSlice | null>(null)
  let items = $state<TreeNodeSummary[]>([])
  let childrenCursor = $state<string | null>(null)
  let selectedRootId = $state<SpaceRootIdV1 | null>(null)
  let hoveredId = $state<string | null>(null)
  let collector = $state<CollectorEntry[]>([])
  let collectorOpen = $state(false)
  let cleanupPlan = $state<CleanupPlanV1 | null>(null)
  let cleanupBusy = $state(false)
  let decision = $state<'denied' | 'cancelled' | null>(null)
  let problem = $state<string | null>(null)
  let phase = $state<'connecting' | 'ready' | 'failed'>('connecting')
  let requestEpoch = 0
  const i18n = provideLensI18n(typeof navigator === 'undefined' ? 'en' : navigator.language)
  function rootLabel(label: string): string {
    return label.startsWith('/') || label.startsWith('\\\\') || /^[A-Za-z]:[\\/]/.test(label)
      ? i18n.t('xross.roots.default') : label
  }
  const status = $derived(scan === null ? null : projectScanStatus(scan))
  const targets = $derived<ScanTarget[]>(roots.map((root) => ({
    id: root.rootId, label: rootLabel(root.label), path: root.rootId,
    kind: root.kind === 'multiFolder' ? 'multi-folder' : root.kind,
    description: '', size: Number(root.sizeBytes ?? '0'), used: root.usedBytes === undefined ? undefined : Number(root.usedBytes),
    source: 'preset', removable: false,
  })))
  const iCloud = $derived(view === null ? null : createICloudEviction(view))
  function modal(node: HTMLDialogElement) {
    node.showModal()
    return { destroy() { if (node.open) node.close() } }
  }

  function describe(error: unknown): string {
    if (error instanceof XrossViewError) {
      if (error.code === 'StaleGeneration') {
        requestEpoch += 1
        scan = null; slice = null; items = []; collector = []; cleanupPlan = null; selectedRootId = null
        phase = 'failed'
        return i18n.t('xross.scan.stale')
      }
      if (error.code === 'HostUnavailable') return i18n.t('xross.unavailable')
      if (error.code === 'IncompatibleContract') return i18n.t('xross.incompatible')
      if (error.code === 'PermissionDenied') return i18n.t('xross.denied')
    }
    return error instanceof Error ? error.message : String(error)
  }

  onMount(() => {
    let active = true
    async function connect() {
      try {
        const candidate = xrossHostFromWindow(window)
        const connected = await connectXrossView(candidate)
        if (!active) return
        view = connected
        i18n.setLocale(connected.context.locale)
        phase = 'ready'
        await loadRoots()
      } catch (error) {
        if (!active) return
        problem = describe(error)
        phase = 'failed'
      }
    }
    void connect()
    return () => { active = false; requestEpoch += 1 }
  })

  async function loadRoots() {
    if (view === null || loadingRoots) return
    loadingRoots = true
    const epoch = requestEpoch
    try {
      const page = await view.listRoots(rootsCursor as never ?? undefined)
      if (epoch !== requestEpoch) return
      roots = [...roots, ...page.items.filter((item) => !roots.some((root) => root.rootId === item.rootId))]
      rootsCursor = page.nextCursor
    } catch (error) { if (epoch === requestEpoch) problem = describe(error) }
    finally { loadingRoots = false }
  }

  async function beginScan(ids: string[]) {
    if (view === null) return
    requestEpoch += 1
    const epoch = requestEpoch
    problem = null
    slice = null
    items = []
    collector = []
    selectedRootId = ids[0] as SpaceRootIdV1
    try {
      const started = await view.startScan(ids)
      if (epoch !== requestEpoch) return
      scan = started
      if (started.state === 'ready') await ready(started, epoch)
      else void watch(started, epoch)
    } catch (error) { if (epoch === requestEpoch) problem = describe(error) }
  }

  async function watch(initial: ScanStatusV1, epoch: number) {
    if (view === null) return
    try {
      for await (const event of view.watchScan(initial.scanId)) {
        if (epoch !== requestEpoch) return
        scan = event.scan
        if (event.scan.state === 'ready') { await ready(event.scan, epoch); return }
        if (event.scan.state === 'failed' || event.scan.state === 'cancelled') return
      }
      const fresh = await view.getScan(initial.scanId)
      if (epoch === requestEpoch) {
        scan = fresh
        if (fresh.state === 'ready') await ready(fresh, epoch)
      }
    } catch (error) { if (epoch === requestEpoch) problem = describe(error) }
  }

  async function ready(readyScan: ScanStatusV1, epoch: number) {
    if (epoch !== requestEpoch || view === null || readyScan.snapshotId === undefined || selectedRootId === null) return
    try { await focusNodeId(readyRootNodeId(readyScan, selectedRootId)) }
    catch (error) { if (epoch === requestEpoch) problem = describe(error) }
  }

  async function focus(node: TreeNodeSummary) {
    const nodeId = parseSpaceLensId('node', node.id)
    if (nodeId === null) { problem = i18n.t('xross.incompatible'); return }
    await focusNodeId(nodeId)
  }

  async function focusNodeId(nodeId: SpaceNodeIdV1) {
    if (view === null || scan?.snapshotId === undefined) return
    const epoch = requestEpoch
    try {
      const [nextSlice, page, ancestors] = await Promise.all([
        view.treeSlice(scan.snapshotId, nodeId),
        view.childrenPage(scan.snapshotId, nodeId),
        view.allAncestors(scan.snapshotId, nodeId),
      ])
      if (epoch !== requestEpoch) return
      const label = roots.find((root) => root.rootId === selectedRootId)?.label ?? ''
      slice = projectTreeSlice(scan, { ...nextSlice, ancestors }, rootLabel(label))
      items = page.items.map((item) => projectNode(item, `${slice!.focusNode.path}/${item.name}`))
      childrenCursor = page.nextCursor
      hoveredId = null
    } catch (error) { if (epoch === requestEpoch) problem = describe(error) }
  }

  async function moreChildren() {
    if (view === null || scan?.snapshotId === undefined || slice === null || childrenCursor === null) return
    const epoch = requestEpoch
    try {
      const page = await view.childrenPage(scan.snapshotId, slice.focusNode.id as SpaceNodeIdV1, childrenCursor as never)
      if (epoch !== requestEpoch) return
      items = [...items, ...page.items.map((item) => projectNode(item, `${slice!.focusNode.path}/${item.name}`))]
      childrenCursor = page.nextCursor
    } catch (error) { if (epoch === requestEpoch) problem = describe(error) }
  }

  function collect(node: TreeNodeSummary) {
    if (scan === null || collector.some((entry) => entry.nodeId === node.id)) return
    if (parseSpaceLensId('node', node.id) === null) return
    collector = [...collector, { id: crypto.randomUUID(), scanId: scan.scanId, nodeId: node.id,
      path: node.path, name: node.name, size: node.size, addedAt: new Date().toISOString() }]
  }

  async function previewCleanup() {
    if (view === null || scan?.snapshotId === undefined || collector.length === 0) return
    const nodeIds = collector.map((entry) => parseSpaceLensId('node', entry.nodeId))
    if (nodeIds.some((id) => id === null)) { problem = i18n.t('xross.incompatible'); return }
    cleanupBusy = true
    cleanupPlan = null
    decision = null
    try { cleanupPlan = await view.previewCleanup(scan.snapshotId, nodeIds as SpaceNodeIdV1[]) }
    catch (error) { problem = describe(error) }
    finally { cleanupBusy = false; collectorOpen = false }
  }

  async function approveCleanup() {
    if (view === null || cleanupPlan === null) return
    cleanupBusy = true
    const plan = cleanupPlan
    cleanupPlan = null
    try {
      const result = await view.requestCleanupApproval(plan.planId)
      if (result.decision === 'approved') collector = []
      else decision = result.decision
    } catch (error) { problem = describe(error) }
    finally { cleanupBusy = false }
  }

  async function cancelScan() {
    if (view === null || scan === null) return
    try { scan = await view.cancelScan(scan.scanId) }
    catch (error) { problem = describe(error) }
  }
</script>

<svelte:head><title>{i18n.t('xross.title')}</title></svelte:head>
{#if phase === 'connecting'}
  <main class="grid h-dvh place-items-center bg-background text-foreground">{i18n.t('xross.page.loading')}</main>
{:else if phase === 'failed'}
  <main class="grid h-dvh place-items-center bg-background p-6 text-foreground"><div class="grid gap-4 text-center"><StateBanner state="error" title={problem === i18n.t('xross.scan.stale') ? problem : i18n.t('xross.unavailable')} detail={problem === i18n.t('xross.unavailable') || problem === i18n.t('xross.scan.stale') ? '' : problem ?? ''} /><button class="rounded border px-3 py-2" onclick={() => location.reload()}>{i18n.t('xross.reload')}</button></div></main>
{:else if scan === null || scan.state === 'failed' || scan.state === 'cancelled'}
  <div class="relative">
    <ScanPicker {targets} mode="xross" busy={false} error={problem} status={status} onScan={beginScan} onCancel={cancelScan} />
    {#if rootsCursor !== null}<button class="absolute bottom-4 left-4 rounded border px-3 py-2" onclick={loadRoots} disabled={loadingRoots}>{i18n.t('xross.page.more')}</button>{/if}
  </div>
{:else if scan.state !== 'ready' || slice === null}
  <main class="grid h-dvh place-items-center bg-background p-6 text-foreground">
    <div class="grid gap-4 text-center"><h1 class="text-xl font-semibold">{i18n.t('xross.scan.loading', { name: rootLabel(roots.find((root) => root.rootId === selectedRootId)?.label ?? '') })}</h1>
      <p>{i18n.t('xross.scan.wait')}</p>
      {#if problem}<StateBanner state="error" title={i18n.t('xross.scan.failed')} detail={problem} />{/if}
      {#if scan.state !== 'ready'}<button class="rounded border px-3 py-2" onclick={cancelScan}>{i18n.t('lens.status.stop')}</button>{/if}
    </div>
  </main>
{:else if status !== null}
  <main class="flex h-dvh min-h-0 flex-col bg-background text-foreground">
    <WorkbenchSurface mode="xross" {status} {slice} {items} {hoveredId} {collector} {collectorOpen} deleting={cleanupBusy} error={problem}
      onHover={(id) => hoveredId = id} onFocus={focus} onCollect={collect} onCancel={cancelScan}
      onOpenCollector={() => collectorOpen = true} onCloseCollector={() => collectorOpen = false}
      onRemoveCollector={(id) => collector = collector.filter((entry) => entry.id !== id)} onDelete={previewCleanup}
      collectorActionLabel={i18n.t('xross.cleanup.preview')} />
    {#if childrenCursor !== null}<button class="mx-auto rounded border px-3 py-2" onclick={moreChildren}>{i18n.t('xross.page.more')}</button>{/if}
    {#if view?.capabilities.providers.iCloud === 'available' && selectedRootId !== null && scan.snapshotId !== undefined}
      <div class="border-t p-3"><button class="rounded border px-3 py-2" onclick={() => iCloud?.preview(selectedRootId!, scan!.snapshotId!)}>{i18n.t('xross.icloud.preview')}</button></div>
    {:else}
      <div class="border-t p-3 text-sm text-muted-foreground">{i18n.t('xross.icloud.unavailable')}</div>
    {/if}
  </main>
{/if}
{#if iCloud?.state.job}
  <aside class="fixed bottom-4 right-4 z-40 grid max-w-sm gap-2 rounded-xl border bg-background p-4 shadow-lg" role="status">
    <strong>{i18n.t('xross.job.state', { state: iCloud.state.job.state })}</strong>
    <span>{i18n.count(iCloud.state.job.completedCount)} / {i18n.count(iCloud.state.job.totalCount)}</span>
    {#if iCloud.state.job.state === 'running'}
      <div class="flex gap-2"><button class="rounded border px-3 py-1" onclick={() => iCloud.control('pause')}>{i18n.t('xross.job.pause')}</button><button class="rounded border px-3 py-1" onclick={() => iCloud.control('cancel')}>{i18n.t('xross.job.cancel')}</button></div>
    {:else if iCloud.state.job.state === 'paused'}
      <div class="flex gap-2"><button class="rounded border px-3 py-1" onclick={() => iCloud.control('resume')}>{i18n.t('xross.job.resume')}</button><button class="rounded border px-3 py-1" onclick={() => iCloud.control('cancel')}>{i18n.t('xross.job.cancel')}</button></div>
    {/if}
  </aside>
{:else if iCloud?.state.decision}
  <div class="fixed bottom-4 right-4 rounded border bg-background p-4" role="status">{i18n.t(iCloud.state.decision === 'denied' ? 'xross.icloud.denied' : 'xross.icloud.cancelled')}</div>
{/if}
{#if iCloud?.state.problem}<div class="fixed bottom-4 right-4 rounded border bg-background p-4" role="alert">{iCloud.state.problem}</div>{/if}

{#if cleanupPlan !== null}
  <dialog use:modal onclose={() => cleanupPlan = null} aria-label={i18n.t('xross.cleanup.preview')} class="fixed inset-0 z-50 m-auto w-[min(100%-2rem,32rem)] max-w-lg rounded-xl border bg-background p-6 text-foreground shadow-xl backdrop:bg-black/55">
    <div class="grid gap-4">
      <h2 class="text-lg font-semibold">{i18n.t('xross.cleanup.preview')}</h2>
      <p>{i18n.t('xross.cleanup.summary', { count: i18n.count(cleanupPlan.selected.length), size: i18n.bytes(cleanupPlan.totalBytes) })}</p>
      <p class="text-sm text-muted-foreground">{i18n.t('xross.plan.expires', { when: i18n.absoluteTime(cleanupPlan.expiresAtUnixMs), relative: i18n.relativeTime(cleanupPlan.expiresAtUnixMs) })}</p>
      <div class="flex justify-end gap-2"><button class="rounded border px-3 py-2" onclick={() => cleanupPlan = null}>{i18n.t('lens.collector.later')}</button><button class="rounded bg-primary px-3 py-2 text-primary-foreground" onclick={approveCleanup}>{i18n.t('xross.cleanup.native')}</button></div>
    </div>
  </dialog>
{/if}
{#if decision !== null}<div class="fixed bottom-4 right-4 rounded border bg-background p-4" role="status">{i18n.t(decision === 'denied' ? 'xross.cleanup.denied' : 'xross.cleanup.cancelled')}</div>{/if}
{#if iCloud?.state.plan}
  <dialog use:modal onclose={() => { if (iCloud) iCloud.state.plan = null }} aria-label={i18n.t('xross.icloud.title')} class="fixed inset-0 z-50 m-auto w-[min(100%-2rem,32rem)] max-w-lg rounded-xl border bg-background p-6 text-foreground shadow-xl backdrop:bg-black/55">
    <div class="grid gap-4">
      <h2 class="text-lg font-semibold">{i18n.t('xross.icloud.title')}</h2>
      <p>{i18n.t('xross.icloud.summary', { count: i18n.count(iCloud.state.plan.selectedCount), size: i18n.bytes(iCloud.state.plan.bytesToEvict) })}</p>
      <p class="text-sm text-muted-foreground">{i18n.t('xross.plan.expires', { when: i18n.absoluteTime(iCloud.state.plan.expiresAtUnixMs), relative: i18n.relativeTime(iCloud.state.plan.expiresAtUnixMs) })}</p>
      <div class="flex justify-end gap-2"><button class="rounded border px-3 py-2" onclick={() => iCloud.state.plan = null}>{i18n.t('lens.collector.later')}</button><button class="rounded bg-primary px-3 py-2 text-primary-foreground" onclick={() => iCloud.requestNativeApproval()}>{i18n.t('xross.icloud.native')}</button></div>
    </div>
  </dialog>
{/if}
