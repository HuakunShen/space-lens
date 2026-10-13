<script lang="ts">
  import { ArrowRight, Check, LoaderCircle, Minus, Trash2 } from '@lucide/svelte'
  import type { AppCleanupPlan, CleanupOutcome } from '@space-lens/contract'
  import { formatBytes } from '../../lib/format'
  import { Button } from '../ui/button/index'
  import * as Sheet from '../ui/sheet/index'

  interface Props {
    open: boolean
    appName: string
    staged: Array<{ path: string; size: number }>
    stagedTotal: number
    plan: AppCleanupPlan | null
    outcome: CleanupOutcome | null
    error: string | null
    planning: boolean
    deleting: boolean
    disabled?: boolean
    cleanupAvailable: boolean
    onClose: () => void
    onRemove: (path: string) => void
    onPlan: () => void
    onConfirm: () => void
    onBackToReview: () => void
    onClear?: () => void
  }
  let {
    open,
    appName,
    staged,
    stagedTotal,
    plan,
    outcome,
    error,
    planning,
    deleting,
    disabled = false,
    cleanupAvailable,
    onClose,
    onRemove,
    onPlan,
    onConfirm,
    onBackToReview,
    onClear,
  }: Props = $props()

  let busy = $derived(planning || deleting || disabled)
  let narrow = $state(false)
  $effect(() => {
    const query = window.matchMedia('(max-width: 760px)')
    const update = (): void => {
      narrow = query.matches
    }
    update()
    query.addEventListener('change', update)
    return () => query.removeEventListener('change', update)
  })
</script>

<Sheet.Root {open} onOpenChange={(value) => { if (!value && !busy) onClose() }}>
  <Sheet.Content class="collector-panel flex w-full flex-col gap-0 sm:max-w-xl" side={narrow ? 'bottom' : 'right'} showCloseButton={!busy}>
    <Sheet.Header class="border-b pb-5">
      <Sheet.Title>{plan ? 'Confirm move to Trash' : 'Review selection'}</Sheet.Title>
      <Sheet.Description
        >{plan
          ? `${plan.entries.length} item${plan.entries.length === 1 ? '' : 's'} · ${formatBytes(plan.totalSize)} in the verified plan.`
          : `${staged.length} item${staged.length === 1 ? '' : 's'} · ${formatBytes(stagedTotal)} selected. Files stay in place until you confirm.`}</Sheet.Description
      >
    </Sheet.Header>
    <div class="collector-scroll">
      {#if outcome}
        <div class="cleanup-outcome" role="status"><Check size={17} /><div><strong>{outcome.trashed.length} items moved to Trash · {formatBytes(outcome.bytesFreed)}</strong><p>Empty Trash when you are ready to reclaim space.</p></div></div>
        {#if outcome.trashed.length > 0}<details class="outcome-details"><summary>Moved paths ({outcome.trashed.length})</summary>{#each outcome.trashed as item}<p>{item.path}</p>{/each}</details>{/if}
        {#each outcome.failed as failure}<div class="inline-error"><strong>{failure.path}</strong><p>{failure.message}</p></div>{/each}
      {/if}
      {#if error}
        <p class="inline-error" role="alert">{error}</p>
        <div class="mb-2">
          <Button variant="outline" size="sm" type="button" disabled={busy} onclick={onPlan}>Plan again</Button>
        </div>
      {/if}
      {#if !cleanupAvailable}<div class="cleanup-readonly"><strong>This connection is read-only.</strong><p>You can discover and review items. To move them to Trash, restart your local host with <code>--allow-cleanup</code>.</p></div>{/if}
      {#if plan}
        <p class="review-notice">Review these exact paths for {appName}. The host will check them again before moving them to your system Trash.</p>
        {#each plan.errors as planError}<p class="inline-error">{planError}</p>{/each}
        {#each plan.entries as entry}<div class="plan-entry"><span>{entry.path}</span><strong>{formatBytes(entry.size)}</strong></div>{/each}
        <div class="confirm-copy">
          <strong>Move {plan.entries.length} item{plan.entries.length === 1 ? '' : 's'} to Trash?</strong>
          <p>These exact paths will be moved to your system Trash — {formatBytes(plan.totalSize)} total. This is trash-only; permanent delete stays CLI-only.</p>
          <p>The host re-checks every path (size + modification time) before moving it. If anything changed since the plan, the whole plan is refused as StalePlan/StaleSnapshot and you plan again — nothing is partially deleted on a mismatch.</p>
          <p>Empty the Trash yourself when ready to reclaim space.</p>
        </div>
      {:else if staged.length === 0}
        <p class="selection-empty">Stage locations from the app drawer to review them here.</p>
      {:else}
        {#each staged as item (item.path)}
          <div class="collector-entry"><div><strong>{item.path.split('/').at(-1) || item.path}</strong><span>{item.path}</span></div><span>{formatBytes(item.size)}</span><Button variant="ghost" size="icon-sm" type="button" disabled={busy} onclick={() => onRemove(item.path)} aria-label={`Remove ${item.path}`}><Minus size={15} /></Button></div>
        {/each}
      {/if}
    </div>
    <Sheet.Footer class="border-t pt-4">
      {#if plan}
        <Button variant="ghost" type="button" onclick={onBackToReview} disabled={busy}>Back to review</Button>
        <Button variant="destructive" type="button" onclick={onConfirm} disabled={deleting || disabled || plan.entries.length === 0 || plan.errors.length > 0 || !cleanupAvailable}>{#if deleting}<LoaderCircle size={16} class="animate-spin" />{:else}<Trash2 size={16} />{/if}{deleting ? 'Moving…' : 'Move to Trash'}</Button>
      {:else}
        <Button variant="outline" type="button" onclick={onClose} disabled={busy}>Back to workspace</Button>
        {#if onClear && staged.length > 0}<Button variant="ghost" type="button" onclick={onClear} disabled={busy}>Clear</Button>{/if}
        <Button type="button" onclick={onPlan} disabled={staged.length === 0 || busy || !cleanupAvailable}>{#if busy}<LoaderCircle size={16} class="animate-spin" />{:else}<ArrowRight size={16} />{/if}{planning ? 'Verifying…' : 'Review cleanup plan'}</Button>
      {/if}
    </Sheet.Footer>
  </Sheet.Content>
</Sheet.Root>
