<script lang="ts">
  /**
   * One process, in detail: the "neohtop" half of the memory view. A dialog,
   * not a drawer, because this is a read-and-decide surface — the command line
   * needs room to wrap, the identity fields sit in a two-column card grid, and
   * the quit actions stay together at the bottom. The table stays the dense
   * scanner; this is the inspector it opens.
   *
   * Props only; the page owns the transport, the plan and the Quit list.
   */
  import type { MemorySnapshot, ProcessGroup, ProcessRow } from '@space-lens/contract'
  import { Check, Copy, Lock } from '@lucide/svelte'
  import { Badge } from '../ui/badge/index'
  import { Button } from '../ui/button/index'
  import * as Dialog from '../ui/dialog/index'
  import { Progress } from '../ui/progress/index'
  import {
    formatBytes,
    formatDuration,
    formatPercentValue,
    formatStartTime,
    runningSeconds,
  } from '../../lib/memory/format.ts'
  import { PROCESS_STATE_LABELS } from '../../lib/memory/labels.ts'
  import { KIND_LABEL, processKey, protectionReason, type StageTarget } from './ProcessTable.svelte'
  import ProcessStateDot from './ProcessStateDot.svelte'

  interface Props {
    snapshot: MemorySnapshot
    /** The process to show, or `null` when the dialog is closed. */
    process: ProcessRow | null
    terminateAvailable?: boolean
    onClose: () => void
    /** Add the process to the client Quit list (never executes). */
    onStage?: (key: string, target: StageTarget) => void
    onQuit?: (process: ProcessRow) => void
    onForceQuit?: (process: ProcessRow) => void
    /** Follow a parent/child row inside the dialog. */
    onOpenProcess?: (process: ProcessRow) => void
    /** Open the group drawer for this process's application. */
    onOpenGroup?: (group: ProcessGroup) => void
  }

  let {
    snapshot,
    process,
    terminateAvailable = false,
    onClose,
    onStage,
    onQuit,
    onForceQuit,
    onOpenProcess,
    onOpenGroup,
  }: Props = $props()

  let copied = $state(false)
  let open = $derived(process !== null)

  let group = $derived.by((): ProcessGroup | null => {
    if (process === null) return null
    return snapshot.groups.find((candidate) => candidate.groupId === process?.groupId) ?? null
  })
  let parent = $derived.by((): ProcessRow | null => {
    if (process === null || process.ppid === null) return null
    return snapshot.processes.find((candidate) => candidate.pid === process?.ppid) ?? null
  })
  let children = $derived.by((): ProcessRow[] => {
    if (process === null) return []
    return snapshot.processes
      .filter((candidate) => candidate.ppid === process?.pid)
      .sort(
        (a, b) =>
          (b.footprintBytes ?? b.residentBytes ?? 0) - (a.footprintBytes ?? a.residentBytes ?? 0) ||
          a.pid - b.pid,
      )
  })
  let started = $derived(process === null ? null : formatStartTime(process.startTimeUs))
  let running = $derived(process === null ? null : runningSeconds(process.startTimeUs, snapshot.sampledAt))
  let reason = $derived(process === null ? null : protectionReason(process.protection))
  let canAct = $derived(process !== null && process.protection === null && terminateAvailable)
  let memoryBytes = $derived(
    process === null
      ? null
      : process.footprintBytes !== null && process.footprintBytes > 0
        ? process.footprintBytes
        : process.residentBytes,
  )
  let cpuBar = $derived(Math.min(100, Math.max(0, process?.cpuPercent ?? 0)))

  async function copyCommand(): Promise<void> {
    if (process?.command === null || process?.command === undefined) return
    try {
      await navigator.clipboard.writeText(process.command)
      copied = true
      window.setTimeout(() => (copied = false), 1_500)
    } catch {
      // Clipboard access can be denied; the command is still selectable.
    }
  }

  function stage(): void {
    if (process === null || onStage === undefined) return
    onStage(processKey(process.pid, process.startTimeUs), {
      kind: 'process',
      pid: process.pid,
      startTimeUs: process.startTimeUs,
      name: process.name,
      groupId: process.groupId,
      footprintBytes: process.footprintBytes,
    })
  }

  const statLabel = 'text-[11px] font-medium tracking-wide text-muted-foreground uppercase'
  const statValue = 'mt-1 text-base font-semibold tabular-nums'
  const cardHeading = 'text-[11px] font-semibold tracking-wide text-muted-foreground uppercase'
  const cardTitle = `border-b px-3 py-2 ${cardHeading}`
  const definitionRow = 'grid grid-cols-[6.5rem_1fr] gap-x-3 gap-y-2 text-sm'
</script>

<Dialog.Root {open} onOpenChange={(value) => { if (!value) onClose() }}>
  <Dialog.Content class="max-w-4xl p-0">
    {#if process}
      <Dialog.Header class="border-b pr-14">
        <div class="flex flex-wrap items-center gap-2">
          <Dialog.Title class="truncate text-lg">{process.name}</Dialog.Title>
          <Badge variant="secondary">{KIND_LABEL[process.kind]}</Badge>
          {#if process.restricted}<Badge variant="outline">Restricted</Badge>{/if}
          {#if reason !== null}
            <Badge variant="destructive" title={reason}><Lock />Protected</Badge>
          {/if}
        </div>
        <Dialog.Description>
          PID {process.pid}{group === null ? '' : ` · ${group.name}`} · {PROCESS_STATE_LABELS[process.state]}
        </Dialog.Description>
      </Dialog.Header>

      <div class="grid grid-cols-2 gap-4 border-b bg-muted/30 px-5 py-4 sm:grid-cols-4">
        <div>
          <p class={statLabel}>PID</p>
          <p class={statValue}>{process.pid}</p>
        </div>
        <div>
          <p class={statLabel}>State</p>
          <p class={`${statValue} flex items-center`}>
            <ProcessStateDot state={process.state} showLabel />
          </p>
        </div>
        <div>
          <p class={statLabel}>CPU</p>
          <p class={statValue}>{process.cpuPercent === null ? '—' : `${formatPercentValue(process.cpuPercent)}%`}</p>
        </div>
        <div>
          <p class={statLabel}>Memory</p>
          <p class={statValue}>{memoryBytes === null ? '—' : formatBytes(memoryBytes)}</p>
        </div>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto" role="region" aria-label={`${process.name} details`}>
        <div class="grid gap-4 p-5 lg:grid-cols-2">
          <section class="overflow-hidden rounded-lg border bg-card/30 lg:col-span-2">
            <div class="flex items-center justify-between gap-2 border-b px-3 py-2">
              <h3 class={cardHeading}>Command</h3>
              {#if process.command !== null}
                <Button variant="ghost" size="xs" onclick={() => void copyCommand()}>
                  {#if copied}<Check class="size-3" /> Copied{:else}<Copy class="size-3" /> Copy{/if}
                </Button>
              {/if}
            </div>
            {#if process.command !== null}
              <code
                class="block max-h-48 overflow-auto p-3 font-mono text-xs whitespace-pre-wrap break-all"
              >{process.command}</code>
            {:else}
              <p class="p-3 text-xs text-muted-foreground">
                The command line is not readable for this process. macOS only exposes another
                account's arguments to an administrator.
              </p>
            {/if}
          </section>

          <section class="overflow-hidden rounded-lg border bg-card/30">
            <h3 class={cardTitle}>Process information</h3>
            <dl class={`${definitionRow} p-3`}>
              <dt class="text-muted-foreground">User</dt>
              <dd class="truncate">{process.user ?? (process.ownedByCurrentUser ? 'You' : 'Another user')}</dd>
              <dt class="text-muted-foreground">Parent</dt>
              <dd class="truncate">
                {#if parent !== null && onOpenProcess}
                  <button
                    type="button"
                    class="underline-offset-4 hover:underline"
                    title={`PID ${parent.pid}`}
                    onclick={() => onOpenProcess?.(parent)}
                  >{parent.name} ({parent.pid})</button
                  >
                {:else}
                  {process.ppid ?? '—'}
                {/if}
              </dd>
              <dt class="text-muted-foreground">Application</dt>
              <dd class="truncate">
                {#if group !== null && onOpenGroup}
                  <button
                    type="button"
                    class="underline-offset-4 hover:underline"
                    onclick={() => onOpenGroup?.(group)}
                  >{group.name}</button
                  >
                {:else}
                  {group?.name ?? '—'}
                {/if}
              </dd>
              {#if group?.bundleId}
                <dt class="text-muted-foreground">Bundle ID</dt>
                <dd class="truncate font-mono text-xs">{group.bundleId}</dd>
              {/if}
              {#if group?.bundlePath}
                <dt class="text-muted-foreground">Path</dt>
                <dd class="truncate font-mono text-xs" title={group.bundlePath}>{group.bundlePath}</dd>
              {/if}
              {#if process.threads !== null}
                <dt class="text-muted-foreground">Threads</dt>
                <dd class="tabular-nums">{process.threads}</dd>
              {/if}
              {#if process.listeningPorts.length > 0}
                <dt class="text-muted-foreground">Listening ports</dt>
                <dd class="flex flex-wrap gap-1">
                  {#each process.listeningPorts as port (port)}
                    <Badge variant="outline" class="font-mono text-xs">:{port}</Badge>
                  {/each}
                </dd>
              {/if}
            </dl>
          </section>

          <section class="overflow-hidden rounded-lg border bg-card/30">
            <h3 class={cardTitle}>Resource usage</h3>
            <div class="flex flex-col gap-3 p-3">
              <div>
                <div class="mb-1 flex items-center justify-between text-sm">
                  <span class="text-muted-foreground">CPU</span>
                  <span class="tabular-nums">
                    {process.cpuPercent === null ? '—' : `${formatPercentValue(process.cpuPercent)}%`}
                  </span>
                </div>
                <Progress value={cpuBar} max={100} class="h-1.5" aria-label="CPU usage" />
              </div>
              <dl class={definitionRow}>
                <dt class="text-muted-foreground">Memory</dt>
                <dd class="tabular-nums">{memoryBytes === null ? '—' : formatBytes(memoryBytes)}</dd>
                <dt class="text-muted-foreground">Real memory</dt>
                <dd class="tabular-nums">
                  {process.residentBytes === null ? '—' : formatBytes(process.residentBytes)}
                </dd>
                <dt class="text-muted-foreground">Virtual size</dt>
                <dd class="tabular-nums">
                  {process.virtualBytes === null ? '—' : formatBytes(process.virtualBytes)}
                </dd>
              </dl>
            </div>
          </section>

          <section class="overflow-hidden rounded-lg border bg-card/30">
            <h3 class={cardTitle}>Timeline</h3>
            <dl class={`${definitionRow} p-3`}>
              <dt class="text-muted-foreground">Started</dt>
              <dd class="tabular-nums">{started ?? 'Unknown'}</dd>
              <dt class="text-muted-foreground">Running for</dt>
              <dd class="tabular-nums">{running === null ? '—' : formatDuration(running)}</dd>
              <dt class="text-muted-foreground">Sampled at</dt>
              <dd class="tabular-nums">{new Date(snapshot.sampledAt).toLocaleTimeString()}</dd>
            </dl>
          </section>

          <section class="overflow-hidden rounded-lg border bg-card/30">
            <h3 class={cardTitle}>Children ({children.length})</h3>
            {#if children.length === 0}
              <p class="p-3 text-xs text-muted-foreground">No child processes in this snapshot.</p>
            {:else}
              <ul role="list" class="max-h-56 divide-y overflow-auto">
                {#each children as child (child.pid)}
                  <li>
                    <button
                      type="button"
                      class="flex w-full items-center justify-between gap-2 px-3 py-2 text-left text-sm hover:bg-muted/50"
                      onclick={() => onOpenProcess?.(child)}
                    >
                      <span class="truncate">{child.name}</span>
                      <span class="flex shrink-0 items-center gap-3 text-xs tabular-nums text-muted-foreground">
                        <span>PID {child.pid}</span>
                        <span>{formatBytes(child.footprintBytes ?? child.residentBytes ?? 0)}</span>
                      </span>
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
          </section>

          {#if reason !== null}
            <section class="rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm lg:col-span-2">
              <p class="flex items-center gap-2 font-medium text-destructive"><Lock class="size-3.5" />Protected</p>
              <p class="mt-1 text-muted-foreground">{reason}</p>
            </section>
          {/if}
        </div>
      </div>

      <Dialog.Footer>
        <Button variant="outline" type="button" onclick={onClose}>Close</Button>
        {#if group !== null && onOpenGroup}
          <Button variant="ghost" type="button" onclick={() => onOpenGroup?.(group)}>App details</Button>
        {/if}
        {#if onStage}
          <Button variant="ghost" type="button" disabled={process.protection !== null} onclick={stage}>
            Add to Quit list
          </Button>
        {/if}
        {#if onForceQuit}
          <Button
            variant="destructive"
            type="button"
            disabled={!canAct}
            onclick={() => process && onForceQuit?.(process)}>Force quit</Button
          >
        {/if}
        {#if onQuit}
          <Button type="button" disabled={!canAct} onclick={() => process && onQuit?.(process)}>Quit</Button>
        {/if}
      </Dialog.Footer>
    {/if}
  </Dialog.Content>
</Dialog.Root>
