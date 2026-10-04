<script lang="ts">
  import { Minus, Trash2, ArrowRight, Check, LoaderCircle } from '@lucide/svelte'
  import type { CollectorEntry } from '../../types'
  import type { CleanupPlan, CleanupOutcome } from '@space-lens/contract'
  import { formatBytes } from '../../lib/format'
  import { Button } from '../ui/button/index'
  import * as Sheet from '../ui/sheet/index'
  interface Props {
    open: boolean
    entries: CollectorEntry[]
    totalSize: number
    deleting: boolean
    planning?: boolean
    plan?: CleanupPlan | null
    outcome?: CleanupOutcome | null
    cleanupAvailable?: boolean
    error?: string | null
    onClose: () => void
    onRemove: (id: string) => void
    onDelete: () => void
    onConfirm?: () => void
    onClear?: () => void
  }
  let { open, entries, totalSize, deleting, planning = false, plan = null, outcome = null,
    cleanupAvailable = true, error = null, onClose, onRemove, onDelete, onConfirm, onClear }: Props = $props()
  let busy = $derived(planning || deleting)
</script>
<Sheet.Root {open} onOpenChange={value => { if (!value && !busy) onClose() }}>
  <Sheet.Content class="collector-panel flex w-full flex-col gap-0 sm:max-w-xl" showCloseButton={!busy}>
    <Sheet.Header class="border-b pb-5">
      <Sheet.Title>{plan ? 'Confirm move to Trash' : 'Review selection'}</Sheet.Title>
      <Sheet.Description>{plan ? `${plan.entries.length} item${plan.entries.length === 1 ? '' : 's'} · ${formatBytes(plan.totalSize)} in the verified plan.` : `${entries.length} item${entries.length === 1 ? '' : 's'} · ${formatBytes(totalSize)} selected. Files stay in place until you confirm.`}</Sheet.Description>
    </Sheet.Header>
    <div class="collector-scroll">
      {#if outcome}
        <div class="cleanup-outcome" role="status"><Check size={17} /><div><strong>{outcome.trashed.length} items moved to Trash · {formatBytes(outcome.bytesFreed)}</strong><p>Empty Trash when you are ready to reclaim space.</p></div></div>
        {#if outcome.trashed.length > 0}<details class="outcome-details"><summary>Moved paths ({outcome.trashed.length})</summary>{#each outcome.trashed as item}<p>{item.path}</p>{/each}</details>{/if}
        {#each outcome.failed as failure}<div class="inline-error"><strong>{failure.path}</strong><p>{failure.message}</p></div>{/each}
      {/if}
      {#if error}<p class="inline-error" role="alert">{error}</p>{/if}
      {#if !cleanupAvailable}<div class="cleanup-readonly"><strong>This connection is read-only.</strong><p>You can discover and review items. To move them to Trash, restart your local host with <code>--allow-cleanup</code>.</p></div>{/if}
      {#if plan}
        <p class="review-notice">Review these exact paths. The host will check them again before moving them to your system Trash.</p>
        {#each plan.errors as error}<p class="inline-error">{error}</p>{/each}
        {#each plan.entries as entry}<div class="plan-entry"><span>{entry.path}</span><strong>{formatBytes(entry.size)}</strong></div>{/each}
      {:else if entries.length === 0}
        <p class="selection-empty">Select files or folders from any view to review them here.</p>
      {:else}
        {#each entries as entry (entry.id)}
          <div class="collector-entry"><div><strong>{entry.name}</strong><span>{entry.path}</span></div><span>{formatBytes(entry.size)}</span><Button variant="ghost" size="icon-sm" type="button" disabled={busy} onclick={() => onRemove(entry.id)} aria-label={`Remove ${entry.name}`}><Minus size={15} /></Button></div>
        {/each}
      {/if}
    </div>
    <Sheet.Footer class="border-t pt-4">
      <Button variant="outline" type="button" onclick={onClose} disabled={busy}>Back to workspace</Button>
      {#if !plan && onClear && entries.length > 0}<Button variant="ghost" type="button" onclick={onClear} disabled={busy}>Clear</Button>{/if}
      {#if plan && onConfirm}
        <Button variant="destructive" type="button" onclick={onConfirm} disabled={deleting || plan.entries.length === 0 || plan.errors.length > 0 || !cleanupAvailable}>{#if deleting}<LoaderCircle size={16} class="animate-spin" />{:else}<Trash2 size={16} />{/if}{deleting ? 'Moving…' : 'Move to Trash'}</Button>
      {:else}
        <Button type="button" onclick={onDelete} disabled={entries.length === 0 || busy || !cleanupAvailable}>{#if busy}<LoaderCircle size={16} class="animate-spin" />{:else}<ArrowRight size={16} />{/if}{planning ? 'Verifying…' : onConfirm ? 'Review cleanup plan' : deleting ? 'Moving…' : 'Move to Trash'}</Button>
      {/if}
    </Sheet.Footer>
  </Sheet.Content>
</Sheet.Root>
