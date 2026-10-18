<script lang="ts">
  import type { WorkbenchService } from '@space-lens/client'
  import type { CollectorEntry, TreeNodeSummary } from '@space-lens/contract'
  import { Button, Input, Sheet, formatBytes } from '@space-lens/web-ui'
  import { describeError, workbench } from '../workbench.svelte.ts'
  import {
    PRESETS,
    createStager,
    lensPortFromService,
    renderMarkdown,
    runAgent,
    type AgentEvent,
    type AgentStats,
    type AiChatPort,
    type AiConfig,
    type CleanupRisk,
    type MemoryPort,
    type Preset,
    type StageMeta,
    type Stager,
  } from '@space-lens/ai-advisor'

  /**
   * Slice C — the AI advisor drawer. The page owns availability (see
   * `aiAvailable`) and supplies the host port; opening it never scans. A preset
   * or question runs the slice-A loop over the active scan and stages
   * candidates into the existing collector through `createStager`. There is no
   * delete path here: the user reviews the collector and presses the existing
   * Clean button.
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
  let settingsOpen = $state(false)

  let question = $state('')
  let running = $state(false)
  let error = $state<string | null>(null)
  let transcript = $state<TranscriptItem[]>([])
  let staged = $state<StagedItem[]>([])
  let stats = $state<AgentStats | null>(null)
  /** Assistant text streamed so far this round; cleared when the round's message lands. */
  let liveText = $state('')
  let controller: AbortController | null = null

  // Plain (non-reactive) guard: the effect re-runs on `open`/`available`/`port`,
  // and setting this must not schedule another pass.
  let configRequested = false
  $effect(() => {
    if (!available || !open || port === null || configRequested) return
    configRequested = true
    void loadConfig()
  })

  const runReady = $derived(memory !== undefined || (workbench.service !== null && workbench.activeScanId !== null))
  const hasKey = $derived(config?.hasKey === true)
  /** The HTTP host owns the config; the settings section is then read-only. */
  const managed = $derived(config?.managed === true)
  const stagedTotal = $derived(staged.reduce((sum, item) => sum + item.entry.size, 0))

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

  function toStaged(entries: readonly CollectorEntry[], meta: ReadonlyMap<string, StageMeta>): StagedItem[] {
    return entries.map((entry) => {
      const detail = meta.get(entry.id)
      return { entry, reason: detail?.reason ?? '', risk: detail?.risk ?? 'confirm' }
    })
  }

  function appendEvent(event: AgentEvent, stager: Stager | null): void {
    if (event.type === 'text-delta') {
      liveText += event.text
    } else if (event.type === 'assistant') {
      liveText = ''
      transcript = [...transcript, { kind: 'assistant', round: event.round, text: event.text, tools: event.toolCalls }]
    } else if (event.type === 'tool') {
      transcript = [...transcript, { kind: 'tool', round: event.round, name: event.name, content: event.content }]
    } else if (event.type === 'staged' && stager !== null) {
      staged = toStaged(event.entries, stager.meta)
    }
  }

  function handleResult(result: Awaited<ReturnType<typeof runAgent>>, stager: Stager | null): void {
    stats = result.stats
    if (result.staged.length > 0 && stager !== null) staged = toStaged(result.staged, stager.meta)
    if (result.endReason !== 'final' && result.text !== '') {
      transcript = [...transcript, { kind: 'notice', text: result.text }]
    }
  }

  async function run(prompt: string): Promise<void> {
    if (running || port === null) return
    running = true
    error = null
    stats = null
    staged = []
    controller = new AbortController()
    transcript = [...transcript, { kind: 'user', text: prompt }]
    const chat = port.chat
    const signal = controller.signal
    try {
      if (memory !== undefined) {
        const result = await runAgent({
          chat,
          memory,
          userMessage: prompt,
          signal,
          onEvent: (event) => appendEvent(event, null),
        })
        handleResult(result, null)
      } else {
        const service = workbench.service
        const scanId = workbench.activeScanId
        if (service === null || scanId === null) return
        const stager = createStager({
          scanId,
          resolve: (nodeId) => resolveNode(service, scanId, nodeId),
          collector: () => workbench.collector,
          setCollector: (entries) => {
            workbench.collector = entries
          },
          newId: () => `col_${crypto.randomUUID().slice(0, 8)}`,
        })
        const result = await runAgent({
          chat,
          lens: lensPortFromService(service, scanId, stager.stage),
          userMessage: prompt,
          // The user's open folder, so the model starts where they are looking.
          focusNodeId: workbench.slice?.focusNode.id,
          signal,
          onEvent: (event) => appendEvent(event, stager),
        })
        handleResult(result, stager)
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

  function tokenLabel(tokens: number): string {
    return tokens >= 1000 ? `${(tokens / 1000).toFixed(1)}k` : String(tokens)
  }

  function riskClass(risk: CleanupRisk): string {
    return risk === 'regenerable'
      ? 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-400'
      : 'bg-amber-500/15 text-amber-700 dark:text-amber-400'
  }
</script>

{#if available}
  <Sheet.Root {open} onOpenChange={(value) => { if (!value) onClose() }}>
    <Sheet.Content class="ai-drawer flex w-full flex-col gap-0 sm:max-w-md">
      <Sheet.Header class="border-b pb-4">
        <Sheet.Title>AI advisor</Sheet.Title>
        <Sheet.Description>
          Presets and questions read this view through bounded tools. Anything proposed is staged for
          your review — nothing is deleted or quit until you confirm.
        </Sheet.Description>
      </Sheet.Header>

      <div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 py-4">
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
                <dl class="flex flex-col gap-1 text-xs">
                  <div class="flex items-baseline justify-between gap-2">
                    <dt class="text-muted-foreground">Base URL</dt>
                    <dd class="truncate font-medium" title={baseUrl || 'not set'}>{baseUrl || 'not set'}</dd>
                  </div>
                  <div class="flex items-baseline justify-between gap-2">
                    <dt class="text-muted-foreground">Model</dt>
                    <dd class="truncate font-medium">{model || 'not set'}</dd>
                  </div>
                  <div class="flex items-baseline justify-between gap-2">
                    <dt class="text-muted-foreground">API key</dt>
                    <dd class={hasKey ? 'text-emerald-600 dark:text-emerald-400' : 'text-muted-foreground'}>
                      {hasKey ? 'configured on the host' : 'no key on the host'}
                    </dd>
                  </div>
                  <div class="flex items-baseline justify-between gap-2">
                    <dt class="text-muted-foreground">Model thinking</dt>
                    <dd class="font-medium">{disableThinking ? 'disabled' : 'enabled'}</dd>
                  </div>
                </dl>
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
                  <span>Disable model thinking — much faster on reasoning models (GLM, DeepSeek). Leave off for endpoints that reject unknown fields.</span>
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

        {#if memory === undefined && !runReady}
          <p class="rounded-md border border-dashed px-3 py-2 text-xs text-muted-foreground">Run a scan first</p>
        {/if}

        <section class="flex flex-col gap-2">
          {#each presets as preset (preset.id)}
            <Button.Root
              variant="outline"
              size="sm"
              class="justify-start"
              disabled={!runReady || running}
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
            placeholder={memory === undefined ? 'Ask about this scan…' : 'Ask about memory…'}
            disabled={!runReady || running}
            aria-label={memory === undefined ? 'Ask the AI about this scan' : 'Ask the AI about memory'}
          />
          {#if running}
            <Button.Root type="button" variant="ghost" size="sm" onclick={cancel}>Stop</Button.Root>
          {:else}
            <Button.Root type="submit" size="sm" disabled={!runReady || question.trim() === ''}>Ask</Button.Root>
          {/if}
        </form>

        {#if error}<p class="text-xs text-destructive" role="alert">{error}</p>{/if}

        {#if staged.length > 0}
          <section class="proposal-card flex flex-col gap-2 rounded-md border p-3">
            <header class="flex items-baseline justify-between gap-2">
              <strong class="text-sm">Proposed for cleanup</strong>
              <span class="text-xs text-muted-foreground">{staged.length} · {formatBytes(stagedTotal)}</span>
            </header>
            {#each staged as item (item.entry.id)}
              <div class="flex items-start gap-2 text-xs" title={item.reason}>
                <span class="mt-0.5 rounded px-1.5 py-0.5 text-[10px] font-medium uppercase {riskClass(item.risk)}"
                  >{item.risk}</span
                >
                <div class="min-w-0 flex-1">
                  <p class="truncate font-medium">{item.entry.name}</p>
                  <p class="truncate text-muted-foreground">{item.reason}</p>
                </div>
                <span class="shrink-0 text-muted-foreground">{formatBytes(item.entry.size)}</span>
              </div>
            {/each}
            {#if onOpenCollector}
              <Button.Root variant="outline" size="sm" onclick={onOpenCollector}>Open Collector</Button.Root>
            {/if}
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

      <Sheet.Footer class="border-t">
        <Button.Root variant="outline" onclick={onClose} disabled={running}>Back to workspace</Button.Root>
      </Sheet.Footer>
    </Sheet.Content>
  </Sheet.Root>
{/if}

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
