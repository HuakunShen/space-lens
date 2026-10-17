<script lang="ts">
  import { onMount } from 'svelte'
  import type { ChildrenPage, CollectorEntry, ScanStatus, TreeSlice, TreeNodeSummary } from '@space-lens/contract'
  import { Button, Input, formatBytes } from '@space-lens/web-ui'
  import {
    PRESETS,
    createStager,
    lensPortFromService,
    readConfig,
    renderMarkdown,
    streamChat,
    runAgent,
    type AgentEvent,
    type AgentStats,
    type AiChatPort,
    type AiConfig,
    type ChatRequest,
    type CleanupRisk,
    type LensService,
    type StageMeta,
    type Stager,
  } from '@space-lens/ai-advisor'

  /**
   * Task V — the VS Code webview's AI advisor, an inline section (no Sheet).
   *
   * The extension host owns the network and the key: this panel only names
   * intentions through the `request` bridge and renders the typed answers. The
   * agent loop and tool layer come from `@space-lens/ai-advisor`; the read-only
   * `LensService` is built over the bridge (`scan.status` / `tree.slice` /
   * `tree.children`), and `discover` is deliberately omitted so
   * `find_cache_candidates` reports that this host cannot run discovery.
   *
   * Staging keeps the collector in a local array — the extension has no
   * collector — and the only action a proposal offers is `reveal`. There is no
   * delete or trash path here.
   */

  /** The webview bridge: posts a request and resolves the whole `{id, ok, answer}` message. */
  type BridgeRequest = <T>(request: Record<string, unknown>) => Promise<T>
  /** Streamed bridge request: raw upstream text chunks arrive through `onChunk`. */
  type BridgeStream = (
    request: Record<string, unknown>,
    onChunk: (text: string) => void,
    signal?: AbortSignal,
  ) => Promise<void>

  interface Props {
    request: BridgeRequest
    requestStream: BridgeStream
    scanId: string
    focusNodeId: string | undefined
    rootPath: string | null
    onClose: () => void
  }
  let { request, requestStream, scanId, focusNodeId, rootPath, onClose }: Props = $props()

  interface StagedItem {
    entry: CollectorEntry
    reason: string
    risk: CleanupRisk
  }

  type TranscriptItem =
    | { kind: 'user'; text: string }
    | { kind: 'assistant'; round: number; text: string | null; tools: string[] }
    | { kind: 'tool'; round: number; name: string; content: string }
    | { kind: 'notice'; text: string }

  let config = $state<AiConfig | null>(null)
  let baseUrl = $state('')
  let model = $state('')
  let apiKey = $state('')
  let disableThinking = $state(false)
  let settingsBusy = $state(false)
  let settingsError = $state<string | null>(null)
  let settingsOpen = $state(true)

  let question = $state('')
  let running = $state(false)
  let error = $state<string | null>(null)
  let transcript = $state<TranscriptItem[]>([])
  let staged = $state<StagedItem[]>([])
  let stats = $state<AgentStats | null>(null)
  /** Assistant text streamed so far this round; cleared when the round's message lands. */
  let liveText = $state('')
  let controller: AbortController | null = null
  let revealErrors = $state<Record<string, string>>({})
  let revealingId = $state<string | null>(null)

  /** The local collector the stager owns; there is no host-side collector in the extension. */
  let collector: CollectorEntry[] = []

  const hasKey = $derived(config?.hasKey === true)
  const managed = $derived(config?.managed === true)
  const stagedTotal = $derived(staged.reduce((sum, item) => sum + item.entry.size, 0))

  function describeError(failed: unknown): string {
    if (typeof failed === 'object' && failed !== null) {
      const message = (failed as { message?: unknown }).message
      if (typeof message === 'string' && message !== '') return message
    }
    return failed instanceof Error ? failed.message : String(failed)
  }

  /**
   * The bridge mapped onto the read-only `LensService` the tool layer reads.
   * `discover` is omitted on purpose: the extension host has no discovery route,
   * so the tool answers "this host cannot run discovery".
   */
  const lensService: LensService = {
    async scanStatus(id: string): Promise<ScanStatus> {
      const reply = await request<{ answer: { status: ScanStatus } }>({ kind: 'scan.status', scanId: id })
      return reply.answer.status
    },
    async treeSlice(body): Promise<TreeSlice> {
      const reply = await request<{ answer: { slice: TreeSlice } }>({ kind: 'tree.slice', ...body })
      return reply.answer.slice
    },
    async children(body): Promise<ChildrenPage> {
      const reply = await request<{ answer: { page: ChildrenPage } }>({ kind: 'tree.children', ...body })
      return reply.answer.page
    },
  }

  const port: AiChatPort = {
    async chat(body: ChatRequest) {
      return streamChat(
        (onChunk) =>
          requestStream({ kind: 'ai.chatStream', messages: body.messages, tools: body.tools }, onChunk, body.signal),
        body.onText,
      )
    },
    async getConfig(): Promise<AiConfig> {
      const reply = await request<{ answer: { config: unknown } }>({ kind: 'ai.config' })
      return readConfig(reply.answer.config)
    },
    async setConfig(input): Promise<AiConfig> {
      const reply = await request<{ answer: { config: unknown } }>({
        kind: 'ai.setConfig',
        ai: {
          baseUrl: input.baseUrl,
          model: input.model,
          ...(input.apiKey === undefined ? {} : { apiKey: input.apiKey }),
          ...(input.disableThinking === undefined ? {} : { disableThinking: input.disableThinking }),
        },
      })
      return readConfig(reply.answer.config)
    },
  }

  onMount(() => {
    void loadConfig()
  })

  async function loadConfig(): Promise<void> {
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
    settingsBusy = true
    settingsError = null
    try {
      const next = await port.setConfig({
        baseUrl: baseUrl.trim(),
        model: model.trim(),
        disableThinking,
        // Empty field keeps whatever key is stored; the write-only form never
        // clears the key.
        ...(apiKey.trim() === '' ? {} : { apiKey: apiKey.trim() }),
      })
      config = next
      baseUrl = next.baseUrl
      model = next.model
      disableThinking = next.disableThinking
      apiKey = ''
    } catch (failed) {
      settingsError = describeError(failed)
    } finally {
      settingsBusy = false
    }
  }

  async function resolveNode(nodeId: string): Promise<TreeNodeSummary | null> {
    try {
      const reply = await request<{ answer: { slice: TreeSlice } }>({
        kind: 'tree.slice',
        scanId,
        nodeId,
        depth: 0,
        maxChildrenPerNode: 1,
      })
      return reply.answer.slice.focusNode
    } catch {
      return null
    }
  }

  function toStaged(entries: readonly CollectorEntry[], meta: ReadonlyMap<string, StageMeta>): StagedItem[] {
    return entries.map((entry) => {
      const detail = meta.get(entry.id)
      return { entry, reason: detail?.reason ?? '', risk: detail?.risk ?? 'confirm' }
    })
  }

  function appendEvent(event: AgentEvent, stager: Stager): void {
    if (event.type === 'text-delta') {
      liveText += event.text
    } else if (event.type === 'assistant') {
      liveText = ''
      transcript = [...transcript, { kind: 'assistant', round: event.round, text: event.text, tools: event.toolCalls }]
    } else if (event.type === 'tool') {
      transcript = [...transcript, { kind: 'tool', round: event.round, name: event.name, content: event.content }]
    } else if (event.type === 'staged') {
      staged = toStaged(event.entries, stager.meta)
    }
  }

  async function run(prompt: string): Promise<void> {
    if (running) return
    running = true
    error = null
    stats = null
    staged = []
    revealErrors = {}
    controller = new AbortController()
    transcript = [...transcript, { kind: 'user', text: prompt }]
    const stager = createStager({
      scanId,
      resolve: resolveNode,
      collector: () => collector,
      setCollector: (entries) => {
        collector = entries
      },
      newId: () => `col_${crypto.randomUUID().slice(0, 8)}`,
    })
    const lens = lensPortFromService(lensService, scanId, stager.stage)
    try {
      const result = await runAgent({
        chat: port.chat,
        lens,
        userMessage: prompt,
        focusNodeId,
        signal: controller.signal,
        onEvent: (event) => appendEvent(event, stager),
      })
      stats = result.stats
      if (result.staged.length > 0) staged = toStaged(result.staged, stager.meta)
      if (result.endReason !== 'final' && result.text !== '') {
        transcript = [...transcript, { kind: 'notice', text: result.text }]
      }
    } catch (failed) {
      error = describeError(failed)
    } finally {
      running = false
      liveText = ''
      controller = null
    }
  }

  function cancel(): void {
    controller?.abort()
  }

  function ask(): void {
    const prompt = question.trim()
    if (prompt === '') return
    question = ''
    void run(prompt)
  }

  async function reveal(item: StagedItem): Promise<void> {
    revealingId = item.entry.id
    const next = { ...revealErrors }
    delete next[item.entry.id]
    revealErrors = next
    try {
      const reply = await request<{ answer: { revealed: boolean } }>({ kind: 'reveal', path: item.entry.path })
      if (!reply.answer.revealed) {
        revealErrors = { ...revealErrors, [item.entry.id]: 'The host refused to reveal this path.' }
      }
    } catch (failed) {
      revealErrors = { ...revealErrors, [item.entry.id]: describeError(failed) }
    } finally {
      revealingId = null
    }
  }

  function tokenLabel(tokens: number): string {
    return tokens >= 1000 ? `${(tokens / 1000).toFixed(1)}k` : String(tokens)
  }

  function riskClass(risk: CleanupRisk): string {
    return risk === 'regenerable'
      ? 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-400'
      : 'bg-amber-500/15 text-amber-700 dark:text-amber-400'
  }
</script>

<div class="flex flex-col gap-4 rounded-md border p-4" aria-label="AI advisor">
  <header class="flex items-start justify-between gap-2">
    <div class="min-w-0">
      <h2 class="text-sm font-semibold">AI advisor</h2>
      <p class="text-xs text-muted-foreground">
        Presets and questions read the active scan through bounded tools; candidates are staged for
        review only. Nothing is deleted from here.
      </p>
      {#if rootPath}<p class="truncate text-xs text-muted-foreground" title={rootPath}>Scan: {rootPath}</p>{/if}
    </div>
    <Button.Root variant="outline" size="sm" onclick={onClose} disabled={running}>Close</Button.Root>
  </header>

  <section class="rounded-md border">
    <button
      type="button"
      class="flex w-full items-center justify-between px-3 py-2 text-sm font-medium"
      aria-expanded={settingsOpen}
      onclick={() => (settingsOpen = !settingsOpen)}
    >
      <span>Model settings</span>
      <span class="text-xs text-muted-foreground">{model || 'not set'}</span>
    </button>
    {#if settingsOpen}
      <div class="flex flex-col gap-2 border-t px-3 py-3">
        {#if managed}
          <p class="text-xs text-muted-foreground">
            The AI endpoint and key are configured on the host. They cannot be changed from here.
          </p>
        {:else}
          <label class="flex flex-col gap-1 text-xs text-muted-foreground">
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
          <label class="flex items-start gap-2 text-xs text-muted-foreground">
            <input type="checkbox" class="mt-0.5" bind:checked={disableThinking} disabled={settingsBusy} />
            <span>Disable model thinking — much faster on reasoning models (GLM, DeepSeek).</span>
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
      </div>
    {/if}
  </section>

  <section class="flex flex-col gap-2">
    {#each PRESETS as preset (preset.id)}
      <Button.Root
        variant="outline"
        size="sm"
        class="justify-start"
        disabled={running}
        onclick={() => void run(preset.prompt)}>{preset.title}</Button.Root
      >
    {/each}
  </section>

  <form
    class="flex items-center gap-2"
    onsubmit={(event) => {
      event.preventDefault()
      ask()
    }}
  >
    <Input.Root
      bind:value={question}
      placeholder="Ask about this scan…"
      disabled={running}
      aria-label="Ask the AI about this scan"
    />
    {#if running}
      <Button.Root type="button" variant="ghost" size="sm" onclick={cancel}>Stop</Button.Root>
    {:else}
      <Button.Root type="submit" size="sm" disabled={question.trim() === ''}>Ask</Button.Root>
    {/if}
  </form>

  {#if error}<p class="text-xs text-destructive" role="alert">{error}</p>{/if}

  {#if staged.length > 0}
    <section class="flex flex-col gap-2 rounded-md border p-3">
      <header class="flex items-baseline justify-between gap-2">
        <strong class="text-sm">Proposed for cleanup</strong>
        <span class="text-xs text-muted-foreground">{staged.length} · {formatBytes(stagedTotal)}</span>
      </header>
      {#each staged as item (item.entry.id)}
        <div class="flex flex-col gap-1 text-xs" title={item.reason}>
          <div class="flex items-start gap-2">
            <span class="mt-0.5 rounded px-1.5 py-0.5 text-[10px] font-medium uppercase {riskClass(item.risk)}"
              >{item.risk}</span
            >
            <div class="min-w-0 flex-1">
              <p class="truncate font-medium">{item.entry.name}</p>
              <p class="truncate text-muted-foreground">{item.reason}</p>
            </div>
            <span class="shrink-0 text-muted-foreground">{formatBytes(item.entry.size)}</span>
            <Button.Root
              variant="ghost"
              size="sm"
              disabled={revealingId === item.entry.id}
              onclick={() => void reveal(item)}>Reveal</Button.Root
            >
          </div>
          {#if revealErrors[item.entry.id]}
            <p class="text-xs text-destructive" role="alert">{revealErrors[item.entry.id]}</p>
          {/if}
        </div>
      {/each}
    </section>
  {/if}

  {#if stats}
    <p class="text-xs text-muted-foreground">
      {stats.rounds} rounds · {stats.toolCalls} tool calls · ~{tokenLabel(stats.approxTokensSent)} tokens · {formatBytes(
        stats.bytesSent,
      )} sent
    </p>
  {/if}

  {#if liveText !== ''}
    <div class="ai-md text-sm" aria-live="polite" aria-label="Assistant is typing">{@html renderMarkdown(liveText)}</div>
  {/if}

  {#if transcript.length > 0}
    <section class="flex flex-col gap-2" aria-label="Transcript">
      {#each transcript as item, index (index)}
        {#if item.kind === 'user'}
          <p class="self-end rounded-md bg-muted px-2.5 py-1.5 text-xs">{item.text}</p>
        {:else if item.kind === 'assistant'}
          {#if item.text}<div class="ai-md text-sm">{@html renderMarkdown(item.text)}</div>{/if}
          {#if item.tools.length > 0}
            <div class="flex flex-wrap gap-1">
              {#each item.tools as tool, toolIndex (toolIndex)}
                <span class="rounded-full border px-2 py-0.5 text-[11px] text-muted-foreground">{tool}</span>
              {/each}
            </div>
          {/if}
        {:else if item.kind === 'tool'}
          <details class="rounded-md border px-2.5 py-1.5 text-xs">
            <summary class="cursor-pointer text-muted-foreground">↳ {item.name}</summary>
            <pre class="mt-1 max-h-48 overflow-auto whitespace-pre-wrap">{item.content}</pre>
          </details>
        {:else}
          <p class="rounded-md border border-dashed px-2.5 py-1.5 text-xs text-muted-foreground">{item.text}</p>
        {/if}
      {/each}
    </section>
  {/if}
</div>

<style>
  .ai-md :global(p) { margin: 0 0 0.5rem; }
  .ai-md :global(ul), .ai-md :global(ol) { margin: 0 0 0.5rem; padding-left: 1.25rem; }
  .ai-md :global(ul) { list-style: disc; }
  .ai-md :global(ol) { list-style: decimal; }
  .ai-md :global(h3), .ai-md :global(h4), .ai-md :global(h5), .ai-md :global(h6) { margin: 0.5rem 0 0.25rem; font-weight: 600; }
  .ai-md :global(code) { font-family: ui-monospace, monospace; font-size: 0.85em; background: var(--muted); border-radius: 4px; padding: 0 0.25rem; }
  .ai-md :global(pre) { overflow-x: auto; background: var(--muted); border-radius: 6px; padding: 0.5rem; margin: 0 0 0.5rem; }
  .ai-md :global(pre code) { background: none; padding: 0; }
  .ai-md :global(table) { width: 100%; border-collapse: collapse; margin: 0 0 0.5rem; font-size: 0.8125rem; }
  .ai-md :global(th), .ai-md :global(td) { border: 1px solid var(--border); padding: 0.25rem 0.5rem; text-align: left; vertical-align: top; }
  .ai-md :global(th) { background: var(--muted); font-weight: 600; }
</style>
