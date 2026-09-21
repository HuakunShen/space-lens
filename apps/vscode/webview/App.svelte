<script lang="ts">
  import { onMount } from 'svelte'
  import { BreadcrumbBar, ChildList, ScanPicker, StateBanner, StatusBar, SunburstChart } from '@space-lens/web-ui'
  import type { ScanStatus, ScanTarget, TreeNodeSummary, TreeSlice } from '@space-lens/contract'

  interface vscodeApi {
    postMessage(message: unknown): void
    getState(): unknown
    setState(state: unknown): void
  }
  declare function acquireVsCodeApi(): vscodeApi

  const vscode = acquireVsCodeApi()

  type Phase = 'pairing' | 'ready' | 'failed'
  let phase = $state<Phase>('pairing')
  let errorMessage = $state<string | null>(null)
  let targets = $state<ScanTarget[]>([])
  let status = $state<ScanStatus | null>(null)
  let slice = $state<TreeSlice | null>(null)
  let items = $state<TreeNodeSummary[]>([])
  let hoveredId = $state<string | null>(null)
  let activeScanId = $state<string | null>(null)

  const collectedIds = $derived(new Set<string>())
  const ancestors = $derived(slice === null ? [] : [...slice.ancestors, slice.focusNode])

  interface Pending {
    resolve: (answer: never) => void
    reject: (problem: { code: string; message: string }) => void
  }
  const pending = new Map<number, Pending>()
  let nextId = 1

  function call(request: Omit<{ id: number; kind: string }, 'id' | 'kind'> & { kind: string }): Promise<never> {
    const id = nextId++
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject } as Pending)
      vscode.postMessage({ id, ...request })
    })
  }

  window.addEventListener('message', (event) => {
    const message = event.data as Record<string, unknown>
    if (message.kind === 'push') {
      status = message.status as ScanStatus
      if (status.state === 'ready' && activeScanId !== null) void loadRoot(activeScanId)
      return
    }
    const id = message.id as number
    const entry = pending.get(id)
    if (entry === undefined) return
    pending.delete(id)
    if (message.ok === true) {
      handleAnswer(message as never)
      ;(entry.resolve as (value: unknown) => void)(message)
    } else {
      ;(entry.reject as (problem: never) => void)(message.problem as never)
    }
  })

  function handleAnswer(answer: { answer: { kind: string } }): void {
    // answers are consumed through the promise; pairing materializes targets
    const payload = (answer as unknown as { answer: Record<string, unknown> }).answer
    if (payload.kind === 'pair') {
      targets = (payload.roots as ScanTarget[]) ?? []
      phase = 'ready'
    }
  }

  async function request<T>(request: Record<string, unknown>): Promise<T> {
    return call(request) as Promise<T>
  }

  async function startScan(paths: string[]): Promise<void> {
    try {
      const session = await request<{ session: { scanId: string } }>({ kind: 'scan.start', paths })
      activeScanId = session.session.scanId
      status = {
        scanId: session.session.scanId,
        state: 'scanning',
        message: '',
        progress: null,
        currentPath: null,
        bytesScanned: 0,
        entriesScanned: 0,
        rootIds: [],
        label: paths[0] ?? null,
        updatedAt: new Date().toISOString(),
      }
      pollUntilReady(session.session.scanId)
    } catch (problem) {
      errorMessage = (problem as { message: string }).message
    }
  }

  function pollUntilReady(scanId: string): void {
    const timer = setInterval(() => {
      void request<{ status: ScanStatus }>({ kind: 'scan.status', scanId })
        .then((answer) => {
          status = answer.status
          if (answer.status.state === 'ready') {
            clearInterval(timer)
            void loadRoot(scanId)
          }
        })
        .catch(() => clearInterval(timer))
    }, 1_500)
  }

  async function loadRoot(scanId: string): Promise<void> {
    try {
      const statusAnswer = await request<{ status: ScanStatus }>({ kind: 'scan.status', scanId })
      const rootId = statusAnswer.status.rootIds[0]
      if (rootId === undefined) return
      const sliceAnswer = await request<{ slice: TreeSlice }>({ kind: 'tree.slice', scanId, nodeId: rootId, depth: 3, maxChildrenPerNode: 50 })
      slice = sliceAnswer.slice
      const children = await request<{ page: { items: TreeNodeSummary[] } }>({ kind: 'tree.children', scanId, nodeId: rootId, offset: 0, limit: 200, sort: 'size' })
      items = children.page.items
    } catch (problem) {
      errorMessage = (problem as { message: string }).message
    }
  }

  async function focus(node: TreeNodeSummary): Promise<void> {
    if (activeScanId === null) return
    try {
      const sliceAnswer = await request<{ slice: TreeSlice }>({ kind: 'tree.slice', scanId: activeScanId, nodeId: node.id, depth: 3, maxChildrenPerNode: 50 })
      slice = sliceAnswer.slice
      const children = await request<{ page: { items: TreeNodeSummary[] } }>({ kind: 'tree.children', scanId: activeScanId, nodeId: node.id, offset: 0, limit: 200, sort: 'size' })
      items = children.page.items
    } catch (problem) {
      errorMessage = (problem as { message: string }).message
    }
  }

  onMount(() => {
    void request({ kind: 'pair', ticket: '' })
      .catch((problem: { message: string }) => {
        phase = 'failed'
        errorMessage = problem.message
      })
  })
</script>

{#if phase === 'pairing'}
  <div class="p-4 text-sm text-neutral-400">Connecting to the local Space Lens service…</div>
{:else if phase === 'failed'}
  <StateBanner state="error" title="Could not reach spacelens serve" detail={errorMessage ?? undefined} />
{:else if status === null || status.state !== 'ready'}
  <div class="p-4">
    <ScanPicker {targets} mode="vscode" busy={status?.state === 'scanning'} error={errorMessage} status={status} onScan={(paths) => void startScan(paths)} onCancel={() => {}} />
  </div>
{:else}
  <div class="flex min-h-screen flex-col gap-3 p-4">
    {#if errorMessage}
      <StateBanner state="error" title="Something failed" detail={errorMessage} />
    {/if}
    <div class="grid flex-1 grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1fr)_320px]">
      <div class="flex min-w-0 flex-col items-center gap-3">
        <SunburstChart tree={slice?.tree ?? null} focusNode={slice?.focusNode ?? null} hoveredId={hoveredId} collectedIds={collectedIds} onHover={(id) => (hoveredId = id)} onOpen={(node) => void focus(node)} onContext={(node) => void focus(node)} />
        <BreadcrumbBar items={ancestors} onSelect={(node) => void focus(node)} />
      </div>
      <ChildList items={items} hoveredId={hoveredId} collectedIds={collectedIds} onHover={(id) => (hoveredId = id)} onOpen={(node) => void focus(node)} onCollect={() => {}} onContext={() => {}} />
    </div>
    <StatusBar {status} collectorTotal={0} collectorCount={0} onOpenCollector={() => {}} onCancel={() => {}} />
  </div>
{/if}
