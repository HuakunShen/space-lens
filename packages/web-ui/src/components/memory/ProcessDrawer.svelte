<script lang="ts">
  /**
   * Per-group drill-down drawer (§6.2 item 5): identity, started time, the
   * member processes, the group's footprint sparkline from `memoryHistory`, the
   * risks a quit carries, and the Quit / Force quit / Add to Quit list actions.
   * The actions are callbacks — the drawer never builds a plan or calls the
   * transport itself.
   */
  import type { MemoryHistory, MemorySnapshot, ProcessGroup, ProcessRow } from '@space-lens/contract'
  import { Button } from '../ui/button/index'
  import { Badge } from '../ui/badge/index'
  import * as Sheet from '../ui/sheet/index'
  import { ScrollArea } from '../ui/scroll-area/index'
  import { formatBytes, formatPercentValue } from '../../lib/memory/format.ts'
  import Sparkline from './Sparkline.svelte'
  import { groupKey, KIND_LABEL, protectionReason, type StageTarget } from './ProcessTable.svelte'

  interface Props {
    snapshot: MemorySnapshot
    /** The open group, or `null` when the drawer is closed. */
    group: ProcessGroup | null
    history?: MemoryHistory | null
    onClose: () => void
    /** Build a graceful plan for the group (page-level). */
    onQuit?: (group: ProcessGroup) => void
    /** Build a force plan for the group (page-level). */
    onForceQuit?: (group: ProcessGroup) => void
    /** Add the group to the client Quit list (never executes). */
    onStage?: (key: string, target: StageTarget) => void
  }

  let {
    snapshot,
    group,
    history = null,
    onClose,
    onQuit,
    onForceQuit,
    onStage,
  }: Props = $props()

  let open = $derived(group !== null)

  let members = $derived.by((): ProcessRow[] => {
    if (!group) return []
    const pids = new Set(group.pids)
    return snapshot.processes
      .filter((process) => pids.has(process.pid))
      .sort(
        (a, b) =>
          (b.footprintBytes ?? b.residentBytes ?? 0) - (a.footprintBytes ?? a.residentBytes ?? 0) ||
          a.pid - b.pid,
      )
  })

  let series = $derived(group ? (history?.groups.find((entry) => entry.groupId === group.groupId) ?? null) : null)

  let risks = $derived.by((): string[] => {
    if (!group) return []
    const list: string[] = []
    if (group.kind === 'app') list.push('macOS will ask this app to save changes before it quits, if it needs to.')
    if (group.kind === 'agent') list.push('Runs in the background without a Dock icon.')
    if (group.pids.length > 1) list.push(`Quitting ends all ${group.pids.length} processes in this app.`)
    const reason = protectionReason(group.protection)
    if (reason !== null) list.push(reason)
    return list
  })

  let protectedGroup = $derived(group !== null && group.protection !== null)
  let canAct = $derived(!protectedGroup && group !== null)

  function memoryText(process: ProcessRow): string {
    if (process.footprintBytes !== null && process.footprintBytes > 0) return formatBytes(process.footprintBytes)
    if (process.residentBytes !== null && process.residentBytes > 0) return `≈ ${formatBytes(process.residentBytes)}`
    return '—'
  }

  function stage(): void {
    if (!group || !onStage) return
    onStage(groupKey(group.groupId), {
      kind: 'group',
      groupId: group.groupId,
      name: group.name,
      footprintBytes: group.footprintBytes,
    })
  }
</script>

<Sheet.Root {open} onOpenChange={(value) => { if (!value) onClose() }}>
  <Sheet.Content class="flex w-full flex-col gap-4 sm:max-w-md">
    {#if group}
      <Sheet.Header class="border-b pb-4">
        <div class="flex items-center gap-2 pr-8">
          <Sheet.Title class="truncate">{group.name}</Sheet.Title>
          <Badge variant="secondary">{KIND_LABEL[group.kind]}</Badge>
          {#if group.frontmost}<Badge variant="outline">Frontmost</Badge>{/if}
        </div>
        <Sheet.Description>
          {group.pids.length} process{group.pids.length === 1 ? '' : 'es'} · started
          {new Date(group.startedAt).toLocaleString()} · {formatBytes(group.footprintBytes)}
        </Sheet.Description>
      </Sheet.Header>

      <ScrollArea class="min-h-0 flex-1" aria-label={`${group.name} details`}>
        <div class="flex flex-col gap-4 px-4 pb-4">
        <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-sm">
          {#if group.bundleId}
            <dt class="text-muted-foreground">Bundle ID</dt>
            <dd class="truncate font-mono text-xs">{group.bundleId}</dd>
          {/if}
          {#if group.bundlePath}
            <dt class="text-muted-foreground">Path</dt>
            <dd class="truncate font-mono text-xs" title={group.bundlePath}>{group.bundlePath}</dd>
          {/if}
          <dt class="text-muted-foreground">CPU</dt>
          <dd class="tabular-nums">
            {group.cpuPercent === null ? '—' : `${formatPercentValue(group.cpuPercent)}%`}
          </dd>
          <dt class="text-muted-foreground">Real memory</dt>
          <dd class="tabular-nums">{formatBytes(group.residentBytes)}</dd>
        </dl>

        {#if series && series.footprintBytes.length > 1}
          <div>
            <p class="mb-1 text-xs text-muted-foreground">Memory over time</p>
            <Sparkline
              values={series.footprintBytes}
              height={48}
              label={`${group.name} memory over time`}
            />
          </div>
        {/if}

        <div>
          <p class="mb-1 text-xs font-medium tracking-wide text-muted-foreground uppercase">Members</p>
          <ul role="list" class="divide-y rounded-md border text-sm">
            {#each members as process (process.pid)}
              <li class="flex items-center justify-between gap-2 px-2 py-1.5">
                <span class="flex min-w-0 items-center gap-2">
                  <span class="truncate">{process.name}</span>
                  {#if process.restricted}<Badge variant="outline">Restricted</Badge>{/if}
                </span>
                <span class="flex shrink-0 items-center gap-3 text-xs tabular-nums text-muted-foreground">
                  <span>{memoryText(process)}</span>
                  <span>{process.cpuPercent === null ? '—' : `${formatPercentValue(process.cpuPercent)}%`}</span>
                </span>
              </li>
            {/each}
          </ul>
        </div>

        {#if risks.length > 0}
          <div>
            <p class="mb-1 text-xs font-medium tracking-wide text-muted-foreground uppercase">Before you quit</p>
            <ul role="list" class="list-disc space-y-0.5 pl-5 text-sm">
              {#each risks as risk}
                <li>{risk}</li>
              {/each}
            </ul>
          </div>
        {/if}
        </div>
      </ScrollArea>

      <Sheet.Footer class="border-t pt-4">
        <Button variant="outline" type="button" onclick={onClose}>Close</Button>
        {#if onStage}
          <Button variant="ghost" type="button" disabled={!group} onclick={stage}>Add to Quit list</Button>
        {/if}
        {#if onForceQuit}
          <Button variant="destructive" type="button" disabled={!canAct} onclick={() => group && onForceQuit?.(group)}>
            Force quit
          </Button>
        {/if}
        {#if onQuit}
          <Button type="button" disabled={!canAct} onclick={() => group && onQuit?.(group)}>Quit</Button>
        {/if}
      </Sheet.Footer>
    {/if}
  </Sheet.Content>
</Sheet.Root>
