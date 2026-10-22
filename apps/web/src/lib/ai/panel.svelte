<script lang="ts">
  import { tick, untrack } from 'svelte'
  import type { WorkbenchService } from '@space-lens/client'
  import type { TreeNodeSummary } from '@space-lens/contract'
  import { Button, Input, Sheet, formatBytes } from '@space-lens/web-ui'
  import { describeError, workbench } from '../workbench.svelte.ts'
  import {
    PRESETS,
    createStager,
    createToolRuntime,
    lensPortFromService,
    renderMarkdown,
    runAgent,
    type AgentEvent,
    type AgentStats,
    type AiChatPort,
    type AiConfig,
    type ChatMessage,
    type CleanupRisk,
    type LensScope,
    type MemoryPort,
    type Preset,
    type Stager,
    type ToolRuntime,
  } from '@space-lens/ai-advisor'

  /**
   * The AI advisor drawer, as a conversation (slice 2). The page
   * owns availability (see `aiAvailable`) and supplies the host port; opening it
   * never scans. Each message runs the agent loop over the folder open in the
   * workbench (or the whole scan), with the earlier turns and one tool runtime
   * kept for the conversation so ids from earlier answers still resolve.
   * Proposals land in the existing collector through `createStager`; the tray
   * at the bottom edits that selection. There is no delete path here: the user
   * reviews the collector and presses the existing Clean button.
   */
  interface Props {
    open: boolean
    /** False in the browser flavor; the drawer renders nothing. */
    available: boolean
    /** The host's config + chat port; null until the page has resolved one. */
    port: AiChatPort | null
    /**
     * When set, the drawer runs the memory half of the advisor (T22): memory
     * tools replace the scan tools, no scan is needed, and presets come from
     * the caller. Staged quits land in the page's Quit list, not the Collector.
     */
    memory?: MemoryPort
    /** Preset buttons; defaults to the disk presets. */
    presets?: readonly Preset[]
    onClose: () => void
    /** Opens the existing CollectorPanel; absent on the Memory view. */
    onOpenCollector?: () => void
  }
  let { open, available, port, memory, presets = PRESETS, onClose, onOpenCollector }: Props = $props()

  /** What one answer did, in order: prose the model wrote and the tools it read. */
  type Block = { kind: 'text'; text: string } | { kind: 'tool'; name: string; content: string }

  interface Turn {
    id: number
    prompt: string
    /** The folder the turn looked at, or null for the whole scan / memory. */
    scope: string | null
    blocks: Block[]
    /** Streamed text of the round in flight; folded into `blocks` when it lands. */
    live: string
    /** The final answer, kept for the next turn's history. */
    answer: string
    status: 'running' | 'done' | 'stopped' | 'error'
    notice: string | null
    stats: AgentStats | null
  }

  /** History is condensed to each turn's question and final answer. */
  const HISTORY_TURNS = 8
  const WIDTH_KEY = 'space-lens.ai.width'
  const MIN_WIDTH = 380
  const MAX_WIDTH = 1100
  const DEFAULT_WIDTH = 560

  // ── settings ────────────────────────────────────────────────────────────
  let config = $state<AiConfig | null>(null)
  let baseUrl = $state('')
  let model = $state('')
  let apiKey = $state('')
  let disableThinking = $state(false)
  let settingsBusy = $state(false)
  let settingsError = $state<string | null>(null)
  let settingsOpen = $state(false)

  // ── conversation ────────────────────────────────────────────────────────
  let question = $state('')
  let turns = $state<Turn[]>([])
  let error = $state<string | null>(null)
  let controller: AbortController | null = null
  let nextTurnId = 1
  /** The scan this conversation belongs to; a new scan starts a new conversation. */
  let conversationScanId: string | null = null
  // Plain fields: the runtime and stager are per conversation, not rendered.
  let runtime: ToolRuntime = createToolRuntime()
  let stager: Stager | null = null
  /** Bumped when the stager's notes change, so the tray re-reads `stager.meta`. */
  let metaVersion = $state(0)
  /** The user's override: false analyses the open folder, true the whole scan. */
  let wholeScan = $state(false)

  // ── layout ──────────────────────────────────────────────────────────────
  let width = $state(loadWidth())
  let viewportWidth = $state(typeof window === 'undefined' ? 1440 : window.innerWidth)
  let scroller = $state<HTMLElement | null>(null)
  let composer = $state<HTMLTextAreaElement | null>(null)
  /** True while the transcript is scrolled to the bottom; new output then follows. */
  let stick = $state(true)
  let trayOpen = $state(true)
  let resize = $state<{ startX: number; startWidth: number } | null>(null)

  // Plain (non-reactive) guard: the effect re-runs on `open`/`available`/`port`,
  // and setting this must not schedule another pass.
  let configRequested = false
  $effect(() => {
    if (!available || !open || port === null || configRequested) return
    configRequested = true
    void loadConfig()
  })

  $effect(() => {
    const scanId = workbench.activeScanId
    if (memory === undefined && scanId !== conversationScanId) {
      conversationScanId = scanId
      untrack(resetConversation)
    }
  })

  $effect(() => {
    const onResize = () => (viewportWidth = window.innerWidth)
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  })

  const running = $derived(turns.at(-1)?.status === 'running')
  const runReady = $derived(memory !== undefined || (workbench.service !== null && workbench.activeScanId !== null))
  const hasKey = $derived(config?.hasKey === true)
  /** The HTTP host owns the config; the settings section is then read-only. */
  const managed = $derived(config?.managed === true)
  const needsSetup = $derived(config !== null && !managed && (!hasKey || model.trim() === ''))
  const maxWidth = $derived(Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, viewportWidth - 48)))
  const drawerWidth = $derived(Math.min(Math.max(width, MIN_WIDTH), maxWidth))
  const expanded = $derived(drawerWidth >= maxWidth)

  /** The folder open in the workbench, when it is not the scan root. */
  const openFolder = $derived.by((): LensScope | null => {
    const slice = workbench.slice
    if (memory !== undefined || slice === null || slice.scanId !== workbench.activeScanId) return null
    if (slice.ancestors.length === 0) return null
    return { nodeId: slice.focusNode.id, path: slice.focusNode.path }
  })
  const scope = $derived(wholeScan ? null : openFolder)

  interface TrayRow {
    id: string
    name: string
    path: string
    size: number
    risk: CleanupRisk | null
    reason: string | null
  }
  const tray = $derived.by((): TrayRow[] => {
    void metaVersion
    if (memory !== undefined) return []
    const scanId = workbench.activeScanId
    return workbench.collector
      .filter((entry) => entry.scanId === scanId)
      .map((entry) => {
        const note = stager?.meta.get(entry.id)
        return {
          id: entry.id,
          name: entry.name,
          path: entry.path,
          size: entry.size,
          risk: note?.risk ?? null,
          reason: note?.reason ?? null,
        }
      })
      .sort((left, right) => right.size - left.size)
  })
  const trayTotal = $derived(tray.reduce((sum, row) => sum + row.size, 0))

  function loadWidth(): number {
    try {
      const saved = Number(localStorage.getItem(WIDTH_KEY))
      return Number.isFinite(saved) && saved >= MIN_WIDTH ? saved : DEFAULT_WIDTH
    } catch {
      return DEFAULT_WIDTH
    }
  }

  function saveWidth(): void {
    try {
      localStorage.setItem(WIDTH_KEY, String(Math.round(width)))
    } catch {
      // A private window or blocked storage just forgets the width.
    }
  }

  async function loadConfig(): Promise<void> {
    if (port === null) return
    settingsBusy = true
    settingsError = null
    try {
      const next = await port.getConfig()
      config = next
      baseUrl = next.baseUrl
      model = next.model
      disableThinking = next.disableThinking
    } catch (failed) {
      settingsError = describeError(failed)
    } finally {
      settingsBusy = false
    }
  }

  async function saveSettings(): Promise<void> {
    if (port === null) return
    settingsBusy = true
    settingsError = null
    try {
      const next = await port.setConfig({
        baseUrl: baseUrl.trim(),
        model: model.trim(),
        disableThinking,
        // Empty field keeps whatever key is stored; the contract clears only on
        // an explicit null, which this write-only form never sends.
        ...(apiKey.trim() === '' ? {} : { apiKey: apiKey.trim() }),
      })
      config = next
      baseUrl = next.baseUrl
      model = next.model
      disableThinking = next.disableThinking
      apiKey = ''
      settingsOpen = false
    } catch (failed) {
      settingsError = describeError(failed)
    } finally {
      settingsBusy = false
    }
  }

  async function resolveNode(service: WorkbenchService, scanId: string, nodeId: string): Promise<TreeNodeSummary | null> {
    try {
      const slice = await service.treeSlice({ scanId, nodeId, depth: 0, maxChildrenPerNode: 1 })
      return slice.focusNode
    } catch {
      return null
    }
  }

  function resetConversation(): void {
    controller?.abort()
    turns = []
    error = null
    runtime = createToolRuntime()
    stager = null
    metaVersion += 1
    stick = true
  }

  function conversationStager(service: WorkbenchService, scanId: string): Stager {
    stager ??= createStager({
      scanId,
      resolve: (nodeId) => resolveNode(service, scanId, nodeId),
      collector: () => workbench.collector,
      setCollector: (entries) => {
        workbench.collector = entries
      },
      newId: () => `col_${crypto.randomUUID().slice(0, 8)}`,
    })
    return stager
  }

  /** Each finished turn's question and answer, oldest first. */
  function historyMessages(): ChatMessage[] {
    return turns
      .filter((turn) => turn.status !== 'running' && turn.answer !== '')
      .slice(-HISTORY_TURNS)
      .flatMap((turn): ChatMessage[] => [
        { role: 'user', content: turn.prompt },
        { role: 'assistant', content: turn.answer },
      ])
  }

  function updateTurn(id: number, change: (turn: Turn) => void): void {
    const turn = turns.find((candidate) => candidate.id === id)
    if (turn !== undefined) change(turn)
    void followOutput()
  }

  function onEvent(id: number, event: AgentEvent): void {
    if (event.type === 'staged') {
      metaVersion += 1
      return
    }
    updateTurn(id, (turn) => {
      if (event.type === 'text-delta') {
        turn.live += event.text
      } else if (event.type === 'assistant') {
        turn.live = ''
        if (event.text !== null && event.text.trim() !== '') turn.blocks.push({ kind: 'text', text: event.text })
      } else if (event.type === 'tool') {
        turn.blocks.push({ kind: 'tool', name: event.name, content: event.content })
      }
    })
  }

  async function run(prompt: string): Promise<void> {
    if (running || port === null || !runReady) return
    error = null
    const id = nextTurnId++
    const history = historyMessages()
    const turnScope = scope
    turns.push({
      id,
      prompt,
      scope: turnScope?.path ?? null,
      blocks: [],
      live: '',
      answer: '',
      status: 'running',
      notice: null,
      stats: null,
    })
    stick = true
    void followOutput()
    controller = new AbortController()
    const signal = controller.signal
    const chat = port.chat
    try {
      let result: Awaited<ReturnType<typeof runAgent>>
      if (memory !== undefined) {
        result = await runAgent({
          chat,
          memory,
          userMessage: prompt,
          history,
          signal,
          onEvent: (event) => onEvent(id, event),
        })
      } else {
        const service = workbench.service
        const scanId = workbench.activeScanId
        if (service === null || scanId === null) throw new Error('Run a scan first.')
        const stage = conversationStager(service, scanId)
        result = await runAgent({
          chat,
          lens: lensPortFromService(service, scanId, stage.stage, {
            ...(turnScope === null ? {} : { scope: turnScope }),
            proposal: stage.proposal,
            unstage: stage.unstage,
          }),
          userMessage: prompt,
          history,
          runtime,
          // Without a scope the model still starts where the user is looking.
          focusNodeId: turnScope?.nodeId ?? workbench.slice?.focusNode.id,
          signal,
          onEvent: (event) => onEvent(id, event),
        })
      }
      updateTurn(id, (turn) => {
        turn.stats = result.stats
        turn.live = ''
        if (result.endReason === 'final') {
          turn.answer = result.text
          turn.status = 'done'
        } else {
          turn.status = signal.aborted ? 'stopped' : 'error'
          turn.notice = result.text === '' ? null : result.text
          // The last prose the model wrote still helps the next question.
          turn.answer = turn.blocks.findLast((block) => block.kind === 'text')?.text ?? ''
        }
      })
      metaVersion += 1
    } catch (failed) {
      updateTurn(id, (turn) => {
        turn.live = ''
        turn.status = signal.aborted ? 'stopped' : 'error'
        turn.notice = describeError(failed)
      })
    } finally {
      controller = null
    }
  }

  function cancel(): void {
    controller?.abort()
  }

  function ask(): void {
    const prompt = question.trim()
    if (prompt === '' || running) return
    question = ''
    void tick().then(() => fitComposer())
    void run(prompt)
  }

  function onComposerKey(event: KeyboardEvent): void {
    // Enter sends; Shift+Enter adds a line; an IME composition owns Enter.
    if (event.key === 'Enter' && !event.shiftKey && !event.isComposing && event.keyCode !== 229) {
      event.preventDefault()
      ask()
    }
  }

  function fitComposer(): void {
    if (composer === null) return
    composer.style.height = 'auto'
    composer.style.height = `${Math.min(composer.scrollHeight, 200)}px`
  }

  function removeFromTray(entryId: string): void {
    workbench.collector = workbench.collector.filter((entry) => entry.id !== entryId)
  }

  // ── stick to bottom ─────────────────────────────────────────────────────
  async function followOutput(): Promise<void> {
    if (!stick) return
    await tick()
    if (scroller !== null && stick) scroller.scrollTop = scroller.scrollHeight
  }

  function onScroll(): void {
    if (scroller === null) return
    stick = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 48
  }

  function jumpToLatest(): void {
    stick = true
    scroller?.scrollTo({ top: scroller.scrollHeight, behavior: 'smooth' })
  }

  // ── resize ──────────────────────────────────────────────────────────────
  function onResizeDown(event: PointerEvent): void {
    if (event.button !== 0) return
    event.preventDefault()
    ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
    resize = { startX: event.clientX, startWidth: drawerWidth }
  }

  function onResizeMove(event: PointerEvent): void {
    if (resize === null) return
    // The drawer grows leftwards: dragging left makes it wider.
    width = Math.min(Math.max(resize.startWidth + resize.startX - event.clientX, MIN_WIDTH), maxWidth)
  }

  function onResizeUp(event: PointerEvent): void {
    if (resize === null) return
    ;(event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId)
    resize = null
    saveWidth()
  }

  function onResizeKey(event: KeyboardEvent): void {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
    event.preventDefault()
    const step = event.shiftKey ? 64 : 16
    width = Math.min(Math.max(drawerWidth + (event.key === 'ArrowLeft' ? step : -step), MIN_WIDTH), maxWidth)
    saveWidth()
  }

  function toggleExpand(): void {
    width = expanded ? DEFAULT_WIDTH : MAX_WIDTH
    saveWidth()
  }

  // ── labels ──────────────────────────────────────────────────────────────
  const TOOL_LABELS: Record<string, string> = {
    get_scan_summary: 'Read the scan summary',
    list_children: 'Listed a folder',
    find_cache_candidates: 'Searched for caches',
    propose_cleanup: 'Added to the selection',
    list_proposal: 'Read the selection',
    remove_from_proposal: 'Removed from the selection',
    get_memory_summary: 'Read the memory summary',
    list_memory_groups: 'Listed app groups',
    list_memory_children: 'Listed processes',
    get_memory_group: 'Read an app group',
    get_memory_ancestry: 'Traced a process',
    propose_quit: 'Proposed quitting',
  }

  function toolLabel(name: string): string {
    return TOOL_LABELS[name] ?? name
  }

  /** Text and tool blocks, with each run of tool blocks folded into one group. */
  type Segment = { kind: 'text'; text: string } | { kind: 'tools'; items: { name: string; content: string }[] }
  function segments(blocks: readonly Block[]): Segment[] {
    const out: Segment[] = []
    for (const block of blocks) {
      if (block.kind === 'text') {
        out.push({ kind: 'text', text: block.text })
        continue
      }
      const last = out.at(-1)
      if (last?.kind === 'tools') last.items.push({ name: block.name, content: block.content })
      else out.push({ kind: 'tools', items: [{ name: block.name, content: block.content }] })
    }
    return out
  }

  function stepsSummary(items: readonly { name: string }[]): string {
    const names = [...new Set(items.map((item) => toolLabel(item.name).toLowerCase()))]
    const head = names.slice(0, 2).join(', ')
    const steps = items.length === 1 ? '1 step' : `${items.length} steps`
    return `${steps} · ${head}${names.length > 2 ? '…' : ''}`
  }

  function shortPath(path: string): string {
    const parts = path.split('/').filter((part) => part !== '')
    return parts.length <= 2 ? path : `…/${parts.slice(-2).join('/')}`
  }

  function tokenLabel(tokens: number): string {
    return tokens >= 1000 ? `${(tokens / 1000).toFixed(1)}k` : String(tokens)
  }

  function statsLine(stats: AgentStats): string {
    return `${stats.rounds} rounds · ${stats.toolCalls} tool calls · ~${tokenLabel(stats.approxTokensSent)} tokens`
  }

  function riskClass(risk: CleanupRisk | null): string {
    if (risk === null) return 'bg-sky-500/15 text-sky-700 dark:text-sky-400'
    return risk === 'regenerable'
      ? 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-400'
      : risk === 'keep'
        ? 'bg-muted text-muted-foreground'
        : 'bg-amber-500/15 text-amber-700 dark:text-amber-400'
  }

  const subjectLabel = $derived(memory === undefined ? 'this scan' : 'memory')
  const placeholder = $derived(
    memory !== undefined
      ? 'Ask about memory…'
      : turns.length === 0
        ? `Ask about ${scope === null ? 'this scan' : shortPath(scope.path)}…`
        : 'Reply, e.g. “keep the Xcode ones”',
  )
</script>

{#snippet icon(path: string)}
  <svg viewBox="0 0 24 24" class="size-4" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {@html path}
  </svg>
{/snippet}

{#if available}
  <Sheet.Root {open} onOpenChange={(value) => { if (!value) onClose() }}>
    <Sheet.Content
      class="ai-drawer flex w-full flex-col gap-0 p-0 data-[side=right]:sm:max-w-none"
      style={`width: min(100vw, ${drawerWidth}px)`}
      showCloseButton={false}
    >
      <!-- A focusable separator is an ARIA widget (it has a value the keys change),
           which svelte's static check does not recognise. -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize the AI advisor"
        aria-valuenow={drawerWidth}
        aria-valuemin={MIN_WIDTH}
        aria-valuemax={maxWidth}
        tabindex="0"
        class="drawer-resizer"
        data-dragging={resize !== null}
        onpointerdown={onResizeDown}
        onpointermove={onResizeMove}
        onpointerup={onResizeUp}
        onpointercancel={onResizeUp}
        ondblclick={toggleExpand}
        onkeydown={onResizeKey}
      ></div>

      <header class="flex flex-col gap-1 border-b px-4 py-3">
        <div class="flex items-center gap-2">
        <div class="min-w-0 flex-1">
          <Sheet.Title class="text-base">AI advisor</Sheet.Title>
          <Sheet.Description class="sr-only">
            Ask about {subjectLabel}. Anything proposed is staged for your review; nothing is deleted or quit until
            you confirm.
          </Sheet.Description>
        </div>
        <div class="flex shrink-0 items-center gap-0.5">
          <Button.Root
            variant="ghost"
            size="sm"
            disabled={turns.length === 0 && error === null}
            onclick={resetConversation}
            title="Start a new conversation">New chat</Button.Root
          >
          <Button.Root
            variant="ghost"
            size="icon-sm"
            onclick={toggleExpand}
            title={expanded ? 'Narrow the drawer' : 'Widen the drawer'}
            aria-label={expanded ? 'Narrow the drawer' : 'Widen the drawer'}
          >
            {#if expanded}
              {@render icon('<path d="m14 10 7-7"/><path d="M20 10h-6V4"/><path d="m3 21 7-7"/><path d="M4 14h6v6"/>')}
            {:else}
              {@render icon('<path d="M15 3h6v6"/><path d="m21 3-7 7"/><path d="m3 21 7-7"/><path d="M9 21H3v-6"/>')}
            {/if}
          </Button.Root>
          <Button.Root
            variant={settingsOpen ? 'secondary' : 'ghost'}
            size="icon-sm"
            onclick={() => (settingsOpen = !settingsOpen)}
            title="Model settings"
            aria-label="Model settings"
            aria-expanded={settingsOpen}
          >
            {@render icon(
              '<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>',
            )}
          </Button.Root>
          <Button.Root variant="ghost" size="icon-sm" onclick={onClose} title="Close" aria-label="Close the AI advisor">
            {@render icon('<path d="M18 6 6 18"/><path d="m6 6 12 12"/>')}
          </Button.Root>
        </div>
        </div>
          {#if memory === undefined}
            <div class="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
              <span>Looking at</span>
              <span
                class="min-w-0 truncate rounded-full border bg-muted/50 px-2 py-0.5 font-medium text-foreground"
                title={scope?.path ?? 'The whole scan'}
                data-testid="ai-scope">{scope === null ? 'the whole scan' : shortPath(scope.path)}</span
              >
              {#if openFolder !== null}
                <button
                  type="button"
                  class="shrink-0 rounded px-1 underline-offset-2 hover:text-foreground hover:underline"
                  onclick={() => (wholeScan = !wholeScan)}
                  >{wholeScan ? `Use ${shortPath(openFolder.path)}` : 'Use whole scan'}</button
                >
              {/if}
            </div>
          {:else}
            <p class="text-xs text-muted-foreground">Looking at memory right now</p>
          {/if}
      </header>

      {#if settingsOpen || needsSetup}
        <section class="flex flex-col gap-2 border-b bg-muted/30 px-4 py-3" aria-label="Model settings">
          <div class="flex items-baseline justify-between gap-2">
            <strong class="text-sm">Model settings</strong>
            {#if needsSetup && !settingsOpen}
              <span class="text-xs text-muted-foreground">Add a model and key to start</span>
            {/if}
          </div>
          {#if managed}
            <p class="text-xs text-muted-foreground">
              The AI endpoint and key are configured on the host. They cannot be changed from here.
            </p>
            <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
              <dt class="text-muted-foreground">Base URL</dt>
              <dd class="truncate font-medium" title={baseUrl || 'not set'}>{baseUrl || 'not set'}</dd>
              <dt class="text-muted-foreground">Model</dt>
              <dd class="truncate font-medium">{model || 'not set'}</dd>
              <dt class="text-muted-foreground">API key</dt>
              <dd class={hasKey ? 'text-emerald-600 dark:text-emerald-400' : 'text-muted-foreground'}>
                {hasKey ? 'configured on the host' : 'no key on the host'}
              </dd>
              <dt class="text-muted-foreground">Model thinking</dt>
              <dd class="font-medium">{disableThinking ? 'disabled' : 'enabled'}</dd>
            </dl>
          {:else}
            <div class="grid gap-2 sm:grid-cols-2">
              <label class="flex flex-col gap-1 text-xs text-muted-foreground sm:col-span-2">
                Base URL
                <Input.Root bind:value={baseUrl} placeholder="https://api.example.com/v1" disabled={settingsBusy} />
              </label>
              <label class="flex flex-col gap-1 text-xs text-muted-foreground">
                Model
                <Input.Root bind:value={model} placeholder="model-name" disabled={settingsBusy} />
              </label>
              <label class="flex flex-col gap-1 text-xs text-muted-foreground">
                API key
                <Input.Root
                  type="password"
                  bind:value={apiKey}
                  placeholder={hasKey ? 'key saved — leave empty to keep' : 'sk-…'}
                  disabled={settingsBusy}
                  autocomplete="off"
                />
              </label>
            </div>
            <label class="flex items-start gap-2 text-xs text-muted-foreground">
              <input type="checkbox" class="mt-0.5" bind:checked={disableThinking} disabled={settingsBusy} />
              <span
                >Disable model thinking — much faster on reasoning models (GLM, DeepSeek). Leave off for endpoints
                that reject unknown fields.</span
              >
            </label>
            <div class="flex items-center justify-between gap-2">
              <span class="text-xs text-muted-foreground">
                {#if hasKey}<span class="text-emerald-600 dark:text-emerald-400">key saved</span>{:else}No key stored{/if}
              </span>
              <Button.Root size="sm" variant="outline" disabled={settingsBusy} onclick={() => void saveSettings()}
                >{settingsBusy ? 'Saving…' : 'Save'}</Button.Root
              >
            </div>
          {/if}
          {#if settingsError}<p class="text-xs text-destructive" role="alert">{settingsError}</p>{/if}
        </section>
      {/if}

      <div class="relative min-h-0 flex-1">
        <div
          bind:this={scroller}
          onscroll={onScroll}
          class="h-full overflow-y-auto px-4 py-4"
          aria-label="Conversation"
          role="log"
          aria-live="polite"
        >
          {#if turns.length === 0}
            <div class="flex flex-col gap-3 py-2">
              <p class="text-sm text-muted-foreground">
                {#if memory === undefined && !runReady}
                  Run a scan first, then ask what is taking the space.
                {:else if memory === undefined}
                  Ask anything about {scope === null ? 'this scan' : shortPath(scope.path)}, or start from one of
                  these. Proposals are staged for review — nothing is deleted until you press Clean.
                {:else}
                  Ask anything about what is using memory, or start from one of these.
                {/if}
              </p>
              <div class="grid gap-2 {drawerWidth >= 640 ? 'grid-cols-2' : ''}">
                {#each presets as preset (preset.id)}
                  <button
                    type="button"
                    class="rounded-lg border px-3 py-2.5 text-left text-sm transition-colors hover:bg-muted disabled:pointer-events-none disabled:opacity-50"
                    disabled={!runReady || running || needsSetup}
                    onclick={() => void run(preset.prompt)}>{preset.title}</button
                  >
                {/each}
              </div>
            </div>
          {/if}

          <div class="flex flex-col gap-5">
            {#each turns as turn (turn.id)}
              <article class="flex flex-col gap-2" data-testid="ai-turn">
                <div class="flex flex-col items-end gap-1">
                  <p class="max-w-[85%] rounded-2xl rounded-br-md bg-primary px-3 py-2 text-sm whitespace-pre-wrap text-primary-foreground">
                    {turn.prompt}
                  </p>
                  {#if turn.scope !== null}
                    <span class="text-[11px] text-muted-foreground" title={turn.scope}>in {shortPath(turn.scope)}</span>
                  {/if}
                </div>
                {#each segments(turn.blocks) as segment, index (index)}
                  {#if segment.kind === 'text'}
                    <div class="ai-md text-sm">{@html renderMarkdown(segment.text)}</div>
                  {:else}
                    <details class="group rounded-md text-xs text-muted-foreground">
                      <summary class="flex cursor-pointer list-none items-center gap-1.5 py-0.5 hover:text-foreground">
                        <span class="inline-block transition-transform group-open:rotate-90">›</span>
                        {stepsSummary(segment.items)}
                      </summary>
                      <ol class="mt-1 flex flex-col gap-1 border-l pl-3">
                        {#each segment.items as item, itemIndex (itemIndex)}
                          <li>
                            <details>
                              <summary class="cursor-pointer hover:text-foreground">{toolLabel(item.name)}</summary>
                              <pre class="mt-1 max-h-48 overflow-auto rounded bg-muted p-2 text-[11px] whitespace-pre">{item.content}</pre>
                            </details>
                          </li>
                        {/each}
                      </ol>
                    </details>
                  {/if}
                {/each}
                {#if turn.live !== ''}
                  <div class="ai-md text-sm">{@html renderMarkdown(turn.live)}</div>
                {/if}
                {#if turn.status === 'running'}
                  <p class="flex items-center gap-2 text-xs text-muted-foreground">
                    <span class="typing" aria-hidden="true"><i></i><i></i><i></i></span>
                    {turn.live !== '' ? 'Writing…' : turn.blocks.length === 0 ? 'Reading the scan…' : 'Working…'}
                  </p>
                {/if}
                {#if turn.notice !== null}
                  <p
                    class="rounded-md border border-dashed px-2.5 py-1.5 text-xs {turn.status === 'error'
                      ? 'border-destructive/50 text-destructive'
                      : 'text-muted-foreground'}"
                    role={turn.status === 'error' ? 'alert' : undefined}
                  >
                    {turn.notice}
                  </p>
                {/if}
                {#if turn.stats !== null}
                  <p class="text-[11px] text-muted-foreground/80">{statsLine(turn.stats)}</p>
                {/if}
              </article>
            {/each}
          </div>
          {#if error}<p class="mt-3 text-xs text-destructive" role="alert">{error}</p>{/if}
        </div>

        {#if !stick && turns.length > 0}
          <button
            type="button"
            class="absolute bottom-3 left-1/2 -translate-x-1/2 rounded-full border bg-background px-3 py-1 text-xs shadow-md hover:bg-muted"
            onclick={jumpToLatest}>↓ Jump to latest</button
          >
        {/if}
      </div>

      {#if tray.length > 0}
        <section class="border-t bg-muted/20" aria-label="Cleanup selection">
          <div class="flex items-center gap-2 px-4 py-2">
            <button
              type="button"
              class="flex min-w-0 flex-1 items-baseline gap-2 text-left whitespace-nowrap"
              aria-expanded={trayOpen}
              onclick={() => (trayOpen = !trayOpen)}
            >
              <span class="inline-block text-xs text-muted-foreground transition-transform {trayOpen ? 'rotate-90' : ''}"
                >›</span
              >
              <strong class="shrink-0 text-sm">Selected for cleanup</strong>
              <span class="truncate text-xs text-muted-foreground" data-testid="ai-tray-total"
                >{tray.length} · {formatBytes(trayTotal)}</span
              >
            </button>
            {#if onOpenCollector}
              <Button.Root variant="outline" size="sm" onclick={onOpenCollector}>Review & clean</Button.Root>
            {/if}
          </div>
          {#if trayOpen}
            <ul class="max-h-48 overflow-y-auto px-2 pb-2">
              {#each tray as row (row.id)}
                <li class="group flex items-center gap-2 rounded px-2 py-1 text-xs hover:bg-muted" title={row.path}>
                  <span class="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium uppercase {riskClass(row.risk)}"
                    >{row.risk ?? 'yours'}</span
                  >
                  <span class="min-w-0 flex-1 truncate">
                    <span class="font-medium">{row.name}</span>
                    {#if row.reason}<span class="text-muted-foreground"> — {row.reason}</span>{/if}
                  </span>
                  <span class="shrink-0 tabular-nums text-muted-foreground">{formatBytes(row.size)}</span>
                  <button
                    type="button"
                    class="shrink-0 rounded px-1 text-muted-foreground opacity-60 group-hover:opacity-100 hover:text-destructive"
                    aria-label={`Remove ${row.name} from the selection`}
                    title="Remove from the selection"
                    onclick={() => removeFromTray(row.id)}>✕</button
                  >
                </li>
              {/each}
            </ul>
          {/if}
        </section>
      {/if}

      <footer class="border-t px-4 pt-2 pb-3">
        {#if turns.length > 0}
          <div class="mb-2 flex gap-1.5 overflow-x-auto pb-0.5">
            {#each presets as preset (preset.id)}
              <button
                type="button"
                class="shrink-0 rounded-full border px-2.5 py-0.5 text-xs text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-50"
                disabled={!runReady || running}
                onclick={() => void run(preset.prompt)}>{preset.title}</button
              >
            {/each}
          </div>
        {/if}
        <form
          class="flex items-end gap-2 rounded-xl border bg-background px-3 py-2 focus-within:ring-2 focus-within:ring-ring/40"
          onsubmit={(event) => {
            event.preventDefault()
            ask()
          }}
        >
          <textarea
            bind:this={composer}
            bind:value={question}
            rows="1"
            class="max-h-[200px] min-h-[1.5rem] flex-1 resize-none bg-transparent text-sm outline-none placeholder:text-muted-foreground"
            {placeholder}
            disabled={!runReady || needsSetup}
            aria-label={memory === undefined ? 'Ask the AI about this scan' : 'Ask the AI about memory'}
            oninput={fitComposer}
            onkeydown={onComposerKey}
          ></textarea>
          {#if running}
            <Button.Root type="button" variant="outline" size="sm" onclick={cancel}>Stop</Button.Root>
          {:else}
            <Button.Root type="submit" size="sm" disabled={!runReady || needsSetup || question.trim() === ''}>Send</Button.Root>
          {/if}
        </form>
        <p class="mt-1 text-[11px] text-muted-foreground/80">Enter to send · Shift+Enter for a new line</p>
      </footer>
    </Sheet.Content>
  </Sheet.Root>
{/if}

<style>
  .drawer-resizer {
    position: absolute;
    top: 0;
    left: -4px;
    z-index: 10;
    width: 8px;
    height: 100%;
    cursor: col-resize;
    touch-action: none;
  }
  .drawer-resizer::after {
    content: '';
    position: absolute;
    top: 0;
    bottom: 0;
    left: 3.5px;
    width: 1px;
    background: transparent;
    transition: background 120ms;
  }
  .drawer-resizer:hover::after,
  .drawer-resizer:focus-visible::after,
  .drawer-resizer[data-dragging='true']::after {
    left: 3px;
    width: 2px;
    background: var(--primary);
  }
  .drawer-resizer:focus-visible {
    outline: none;
  }

  .typing {
    display: inline-flex;
    gap: 3px;
  }
  .typing i {
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: currentColor;
    animation: typing 1s infinite ease-in-out;
  }
  .typing i:nth-child(2) { animation-delay: 0.15s; }
  .typing i:nth-child(3) { animation-delay: 0.3s; }
  @keyframes typing {
    0%, 80%, 100% { opacity: 0.25; }
    40% { opacity: 1; }
  }

  .ai-md :global(p) { margin: 0 0 0.5rem; }
  .ai-md :global(p:last-child) { margin-bottom: 0; }
  .ai-md :global(ul), .ai-md :global(ol) { margin: 0 0 0.5rem; padding-left: 1.25rem; }
  .ai-md :global(ul) { list-style: disc; }
  .ai-md :global(ol) { list-style: decimal; }
  .ai-md :global(h3), .ai-md :global(h4), .ai-md :global(h5), .ai-md :global(h6) { margin: 0.5rem 0 0.25rem; font-weight: 600; }
  .ai-md :global(code) { font-family: ui-monospace, monospace; font-size: 0.85em; background: var(--muted); border-radius: 4px; padding: 0 0.25rem; overflow-wrap: anywhere; }
  .ai-md :global(pre) { overflow-x: auto; background: var(--muted); border-radius: 6px; padding: 0.5rem; margin: 0 0 0.5rem; }
  .ai-md :global(pre code) { background: none; padding: 0; overflow-wrap: normal; }
  /* A wide table scrolls inside itself instead of widening the drawer. */
  .ai-md :global(table) { display: block; max-width: 100%; overflow-x: auto; border-collapse: collapse; margin: 0 0 0.5rem; font-size: 0.8125rem; }
  .ai-md :global(th), .ai-md :global(td) { border: 1px solid var(--border); padding: 0.25rem 0.5rem; text-align: left; vertical-align: top; }
  .ai-md :global(td) { min-width: 4rem; }
  .ai-md :global(td code) { white-space: nowrap; overflow-wrap: normal; }
  .ai-md :global(th) { background: var(--muted); font-weight: 600; white-space: nowrap; }
</style>
