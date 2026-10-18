<script lang="ts">
  /**
   * Collapsible terminate audit log (§6.2 item 7): the last 50 records, newest
   * first, each with a status chip, who staged it (You / Free up / AI), the
   * method, and — on the desktop host, where `capabilities.memory.reopen` is
   * true — a Reopen button that resolves the bundle id from the record alone.
   *
   * Props only; no transport.
   */
  import type { AuditLog } from '@space-lens/contract'
  import { ChevronDown, ChevronRight, RotateCw } from '@lucide/svelte'
  import { Badge } from '../ui/badge/index'
  import { Button } from '../ui/button/index'
  import { formatBytes } from '../../lib/memory/format.ts'
  import { METHOD_LABEL, ORIGIN_LABEL, STATUS_LABEL, STATUS_VARIANT } from './TerminateDialog.svelte'

  interface Props {
    log: AuditLog | null
    /** Desktop only: `capabilities.memory.reopen` (§4.4). */
    reopenAvailable?: boolean
    onReopen?: (auditId: string) => void
    /** The audit id whose reopen request is in flight, if any. */
    reopenBusyId?: string | null
    error?: string | null
    class?: string
  }

  let {
    log,
    reopenAvailable = false,
    onReopen,
    reopenBusyId = null,
    error = null,
    class: className = '',
  }: Props = $props()

  let open = $state(false)
  let records = $derived((log?.records ?? []).slice(0, 50))
</script>

<div class={`memory-audit ${className}`}>
  <Button variant="ghost" size="sm" type="button" aria-expanded={open} onclick={() => (open = !open)}>
    {#if open}<ChevronDown size={15} />{:else}<ChevronRight size={15} />{/if}
    Audit log
    <span class="text-muted-foreground">({records.length})</span>
  </Button>

  {#if open}
    <div class="mt-2 flex flex-col gap-2">
      {#if error}
        <p class="text-sm text-destructive" role="alert">{error}</p>
      {/if}

      {#if records.length === 0}
        <p class="text-sm text-muted-foreground">Nothing has been quit from Space Lens yet.</p>
      {:else}
        <ul role="list" class="divide-y rounded-md border text-sm">
          {#each records as record (record.auditId)}
            <li class="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2">
              <span class="truncate font-medium">{record.name}</span>
              <Badge variant={STATUS_VARIANT[record.status]}>{STATUS_LABEL[record.status]}</Badge>
              <Badge variant="outline">{ORIGIN_LABEL[record.origin]}</Badge>
              <span class="text-xs text-muted-foreground">{METHOD_LABEL[record.method]}</span>
              <span class="text-xs tabular-nums text-muted-foreground">
                {record.footprintBytes === null ? '—' : formatBytes(record.footprintBytes)}
              </span>
              <span class="ml-auto text-xs text-muted-foreground">{new Date(record.at).toLocaleString()}</span>
              {#if reopenAvailable && onReopen && record.bundleId !== null}
                <Button
                  variant="ghost"
                  size="xs"
                  type="button"
                  disabled={reopenBusyId === record.auditId}
                  onclick={() => onReopen?.(record.auditId)}
                >
                  <RotateCw size={13} />
                  Reopen
                </Button>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</div>
