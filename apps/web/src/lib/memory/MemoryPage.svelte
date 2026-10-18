<script lang="ts">
  /**
   * The Memory workbench page (T20, §6.1–§6.4). It owns every effect the
   * presentational components deliberately do not: the snapshot poller, the
   * table query, the selection, the in-memory Quit list, the `neverSuggest`
   * list (`localStorage`, guarded), and the plan/execute calls. The plan and
   * execute flow always goes through the host — this page never decides safety.
   */
  import { onMount } from 'svelte'
  import type {
    AuditLog,
    Capabilities,
    FreeUpCandidates,
    MemoryHistory,
    MemorySnapshot,
    ProcessGroup,
    TerminateOutcome,
    TerminatePlan,
    TerminatePlanRequest,
    TerminateResult,
    TerminateTarget,
  } from '@space-lens/contract'
  import type { WorkbenchService } from '@space-lens/client'
  import {
    AuditLogPanel,
    Button,
    FreeUpDialog,
    MemoryBreakdownBar,
    MemoryPressureCard,
    ProcessDrawer,
    ProcessSegmentBar,
    ProcessTable,
    QuitListPanel,
    StateBanner,
    TerminateDialog,
  } from '@space-lens/web-ui'
  import type { StageTarget, TableQuery } from '@space-lens/web-ui'
  import { memoryTerminateAvailable } from '../gates.ts'
  import { describeError } from '../workbench.svelte.ts'
  import { addToQuitList, type QuitListEntry, type TerminateOrigin } from './quit-list.ts'
  import { createPoller } from './poller.ts'

  /** The contract ships the enum as a literal array, so derive the union. */
  type TerminateMode = TerminatePlan['mode']

  /**
   * The Quit list's origin for the plan: shared by every entry, else `manual`.
   * (§6.2 item 6; the list badge keeps each entry's own origin.)
   */
  function planOrigin(entries: readonly QuitListEntry[]): TerminateOrigin {
    const first = entries[0]?.origin ?? 'manual'
    return entries.every((entry) => entry.origin === first) ? first : 'manual'
  }

  /**
   * The list `key` grammar comes from `ProcessTable` — `group:<groupId>` or
   * `proc:<pid>:<startUs>` — so a process entry plans as a single-process
   * target and everything else as a group target. Duplicate targets collapse
   * and the ≤ 20 target limit (§5 rule S11) is applied here.
   */
  function planTargets(entries: readonly QuitListEntry[]): TerminateTarget[] {
    const seen = new Set<string>()
    const targets: TerminateTarget[] = []
    for (const entry of entries) {
      if (entry.key.startsWith('proc:')) {
        const [, pidRaw, startRaw] = entry.key.split(':')
        const pid = Number.parseInt(pidRaw ?? '', 10)
        const startTimeUs = Number.parseInt(startRaw ?? '', 10)
        const id = `${pid}:${startTimeUs}`
        if (Number.isNaN(pid) || Number.isNaN(startTimeUs) || seen.has(id)) continue
        seen.add(id)
        targets.push({ kind: 'process', pid, startTimeUs })
      } else {
        if (seen.has(entry.groupId)) continue
        seen.add(entry.groupId)
        targets.push({ kind: 'group', groupId: entry.groupId })
      }
    }
    return targets.slice(0, 20)
  }

  interface Props {
    service: WorkbenchService | null
    capabilities: Capabilities | null
  }

  let { service, capabilities }: Props = $props()

  /** §7: the client's never-suggest list, one `localStorage` key. */
  const NEVER_SUGGEST_KEY = 'spacelens.memory.neverSuggest'
  const NEVER_SUGGEST_CAP = 200

  function loadNeverSuggest(): string[] {
    try {
      const raw = window.localStorage.getItem(NEVER_SUGGEST_KEY)
      if (raw === null) return []
      const parsed: unknown = JSON.parse(raw)
      return Array.isArray(parsed) ? parsed.filter((value): value is string => typeof value === 'string') : []
    } catch {
      return []
    }
  }

  function saveNeverSuggest(ids: readonly string[]): void {
    try {
      window.localStorage.setItem(NEVER_SUGGEST_KEY, JSON.stringify(ids.slice(0, NEVER_SUGGEST_CAP)))
    } catch {
      // Storage unavailable — the list still works for this session.
    }
  }

  let snapshot = $state<MemorySnapshot | null>(null)
  let history = $state<MemoryHistory | null>(null)
  let error = $state<string | null>(null)
  let paused = $state(false)
  const INITIAL_INTERVAL_MS = 1000
  let intervalMs = $state(INITIAL_INTERVAL_MS)
  let query = $state<TableQuery>({ sort: 'footprint', desc: true, filter: 'all', search: '' })
  let selection = $state<Set<string>>(new Set())
  let openGroup = $state<ProcessGroup | null>(null)
  let quitList = $state<QuitListEntry[]>([])
  let quitListOpen = $state(false)
  let freeUpOpen = $state(false)
  let candidates = $state<FreeUpCandidates | null>(null)
  let candidatesLoading = $state(false)
  let plan = $state<TerminatePlan | null>(null)
  let outcome = $state<TerminateOutcome | null>(null)
  let planError = $state<string | null>(null)
  let building = $state(false)
  let executing = $state(false)
  let auditLog = $state<AuditLog | null>(null)
  let auditError = $state<string | null>(null)
  let reopenBusyId = $state<string | null>(null)
  let neverSuggest = $state<string[]>([])
  let measuredBefore = $state<number | null>(null)
  let measuredAfter = $state<number | null>(null)
  let measuredSeconds = $state<number | null>(null)
  let measureStartedAt = 0
  let measuring = false
  let historyBusy = false
  let lastHistoryFetch = 0
  /** The header sparkline does not need 1 Hz detail; refetch at most every 4 s. */
  const HISTORY_MIN_INTERVAL_MS = 4000

  const terminateAvailable = $derived(
    memoryTerminateAvailable({ capabilities, service }),
  )
  const reopenAvailable = $derived(Boolean(capabilities?.memory?.reopen))

  async function refreshHistory(force = false): Promise<void> {
    const memoryHistory = service?.memoryHistory?.bind(service)
    if (memoryHistory === undefined || historyBusy) return
    const now = Date.now()
    if (!force && now - lastHistoryFetch < HISTORY_MIN_INTERVAL_MS) return
    historyBusy = true
    lastHistoryFetch = now
    try {
      const groupIds = openGroup === null ? [] : [openGroup.groupId]
      history = await memoryHistory({ groupIds })
    } catch {
      // History is best-effort; the live table stays usable without it.
    } finally {
      historyBusy = false
    }
  }

  async function refreshAudit(): Promise<void> {
    const memoryAudit = service?.memoryAudit?.bind(service)
    if (memoryAudit === undefined) return
    try {
      auditLog = await memoryAudit()
      auditError = null
    } catch (caught) {
      auditError = describeError(caught)
    }
  }

  function fetchSnapshot(): Promise<MemorySnapshot> {
    const read = service?.memorySnapshot
    if (read === undefined) return Promise.reject(new Error('Memory sampling is not available on this host'))
    return read.call(service)
  }

  const poller = createPoller(
    {
      fetch: fetchSnapshot,
      setTimer: (fn, ms) => window.setTimeout(fn, ms),
      clearTimer: (handle) => window.clearTimeout(handle as number),
      now: () => Date.now(),
      isHidden: () => document.hidden,
      onVisibilityChange: (cb) => {
        document.addEventListener('visibilitychange', cb)
        return () => document.removeEventListener('visibilitychange', cb)
      },
    },
    {
      intervalMs: INITIAL_INTERVAL_MS,
      onSnapshot: (next) => {
        snapshot = next
        error = null
        if (measuring) {
          measuredAfter = next.breakdown.usedBytes
          measuredSeconds = Math.max(0, Math.round((Date.now() - measureStartedAt) / 1000))
          measuring = false
        }
        void refreshHistory()
      },
      onError: (caught) => {
        error = describeError(caught)
      },
    },
  )

  onMount(() => {
    neverSuggest = loadNeverSuggest()
    poller.start()
    void refreshAudit()
    return () => poller.stop()
  })

  function togglePause(): void {
    paused = !paused
    if (paused) poller.stop()
    else poller.start()
  }

  function changeInterval(ms: number): void {
    intervalMs = ms
    poller.setInterval(ms)
  }

  function toggleSelect(key: string): void {
    const next = new Set(selection)
    if (next.has(key)) next.delete(key)
    else next.add(key)
    selection = next
  }

  function openDrawer(group: ProcessGroup): void {
    openGroup = group
    void refreshHistory(true)
  }

  function stage(key: string, target: StageTarget): void {
    const entry: QuitListEntry = {
      key,
      groupId: target.groupId,
      name: target.name,
      origin: 'manual',
      reason: null,
      stagedAt: new Date().toISOString(),
    }
    quitList = addToQuitList(quitList, entry)
  }

  function removeFromQuitList(key: string): void {
    quitList = quitList.filter((entry) => entry.key !== key)
  }

  function rememberNeverSuggest(groupId: string): void {
    if (neverSuggest.includes(groupId)) return
    neverSuggest = [...neverSuggest, groupId]
    saveNeverSuggest(neverSuggest)
  }

  async function openFreeUp(): Promise<void> {
    freeUpOpen = true
    candidates = null
    candidatesLoading = true
    const memoryCandidates = service?.memoryCandidates?.bind(service)
    if (memoryCandidates === undefined) {
      candidatesLoading = false
      return
    }
    try {
      candidates = await memoryCandidates({ exclude: neverSuggest })
    } catch (caught) {
      error = describeError(caught)
      freeUpOpen = false
    } finally {
      candidatesLoading = false
    }
  }

  async function buildPlan(
    origin: TerminateOrigin,
    targets: readonly TerminateTarget[],
    mode: TerminateMode,
  ): Promise<void> {
    const memoryTerminatePlan = service?.memoryTerminatePlan?.bind(service)
    if (memoryTerminatePlan === undefined || targets.length === 0) return
    building = true
    planError = null
    outcome = null
    measuredBefore = null
    measuredAfter = null
    measuredSeconds = null
    const request: TerminatePlanRequest = { targets: [...targets], mode, origin }
    try {
      plan = await memoryTerminatePlan(request)
    } catch (caught) {
      planError = describeError(caught)
      plan = null
    } finally {
      building = false
    }
  }

  function reviewQuitList(): void {
    quitListOpen = false
    void buildPlan(planOrigin(quitList), planTargets(quitList), 'graceful')
  }

  function reviewFreeUp(groupIds: string[]): void {
    freeUpOpen = false
    void buildPlan(
      'freeUp',
      groupIds.slice(0, 20).map((groupId) => ({ kind: 'group' as const, groupId })),
      'graceful',
    )
  }

  function quitGroup(group: ProcessGroup, mode: TerminateMode): void {
    openGroup = null
    void buildPlan('manual', [{ kind: 'group', groupId: group.groupId }], mode)
  }

  function forceQuitProcess(result: TerminateResult): void {
    outcome = null
    plan = null
    void buildPlan('manual', [{ kind: 'process', pid: result.pid, startTimeUs: result.startTimeUs }], 'force')
  }

  async function confirmPlan(current: TerminatePlan): Promise<void> {
    const memoryTerminateExecute = service?.memoryTerminateExecute?.bind(service)
    if (memoryTerminateExecute === undefined) return
    executing = true
    planError = null
    measuredBefore = snapshot?.breakdown.usedBytes ?? null
    measuredAfter = null
    measuredSeconds = null
    measureStartedAt = Date.now()
    measuring = true
    try {
      outcome = await memoryTerminateExecute({ planId: current.planId, confirm: true })
      void refreshAudit()
    } catch (caught) {
      planError = describeError(caught)
      plan = null
      measuring = false
    } finally {
      executing = false
    }
  }

  async function reopenApp(auditId: string): Promise<void> {
    const reopen = service?.reopenApp?.bind(service)
    if (reopen === undefined) return
    reopenBusyId = auditId
    try {
      await reopen({ auditId })
    } catch (caught) {
      auditError = describeError(caught)
    } finally {
      reopenBusyId = null
    }
  }
</script>

{#if snapshot === null}
  <section
    class="flex h-dvh min-h-0 flex-col items-center justify-center gap-4 bg-background p-6 text-foreground"
  >
    {#if error}
      <StateBanner state="error" title="Could not read memory" detail={error} />
      <Button.Root
        variant="secondary"
        size="sm"
        onclick={() => {
          poller.stop()
          poller.start()
        }}>Retry</Button.Root
      >
    {:else}
      <p class="text-sm text-muted-foreground">Reading memory…</p>
    {/if}
  </section>
{:else}
  <main class="memory-page flex h-dvh min-h-0 flex-col gap-4 overflow-auto bg-background p-4 text-foreground">
    <div class="flex flex-wrap items-start gap-4">
      <div class="min-w-0 flex-1">
        <MemoryPressureCard {snapshot} {history} />
      </div>
      <div class="flex min-w-0 flex-1 flex-col gap-3">
        <MemoryBreakdownBar breakdown={snapshot.breakdown} />
        <div class="flex flex-wrap items-center gap-2">
          {#if terminateAvailable}
            <Button.Root onclick={() => void openFreeUp()}>Free up memory…</Button.Root>
            <Button.Root variant="outline" onclick={() => (quitListOpen = true)}>
              Quit list ({quitList.length})
            </Button.Root>
          {:else}
            <p class="text-sm text-muted-foreground">This host does not allow process control.</p>
          {/if}
          {#if paused}<span class="text-xs text-muted-foreground">Paused</span>{/if}
        </div>
      </div>
    </div>

    {#if error}
      <StateBanner
        state="error"
        title="Memory sampling failed"
        detail={`${error} — last reading ${new Date(snapshot.sampledAt).toLocaleTimeString()}`}
      />
    {/if}

    <ProcessSegmentBar
      groups={snapshot.groups}
      usedBytes={snapshot.breakdown.usedBytes}
      {selection}
      onSelect={toggleSelect}
    />

    <ProcessTable
      {snapshot}
      {query}
      {selection}
      onSelect={toggleSelect}
      onOpen={openDrawer}
      onStage={terminateAvailable ? stage : undefined}
      onQueryChange={(next) => (query = next)}
      {intervalMs}
      onIntervalChange={changeInterval}
      {paused}
      onTogglePause={togglePause}
    />

    <AuditLogPanel
      log={auditLog}
      reopenAvailable={reopenAvailable && terminateAvailable}
      onReopen={(auditId) => void reopenApp(auditId)}
      {reopenBusyId}
      error={auditError}
    />

    {#if !terminateAvailable}
      <p class="text-xs text-muted-foreground" role="status">
        This host lists processes read-only. Quitting runs in the desktop app or a host started
        with process control enabled.
      </p>
    {/if}
  </main>

  <ProcessDrawer
    {snapshot}
    group={openGroup}
    {history}
    onClose={() => (openGroup = null)}
    onQuit={(group) => quitGroup(group, 'graceful')}
    onForceQuit={(group) => quitGroup(group, 'force')}
    onStage={terminateAvailable ? stage : undefined}
  />

  <QuitListPanel
    open={quitListOpen}
    entries={quitList}
    onClose={() => (quitListOpen = false)}
    onRemove={removeFromQuitList}
    onReview={reviewQuitList}
    onClear={() => (quitList = [])}
    planning={building}
    {terminateAvailable}
  />

  <FreeUpDialog
    open={freeUpOpen}
    {candidates}
    onReview={reviewFreeUp}
    onClose={() => (freeUpOpen = false)}
    onNeverSuggest={rememberNeverSuggest}
    planning={building || candidatesLoading}
  />

  <TerminateDialog
    {plan}
    {outcome}
    error={planError}
    {executing}
    onConfirm={(current) => void confirmPlan(current)}
    onForceQuit={forceQuitProcess}
    onClose={() => {
      plan = null
      outcome = null
      planError = null
    }}
    measuredBeforeBytes={measuredBefore}
    measuredAfterBytes={measuredAfter}
    {measuredSeconds}
  />
{/if}
