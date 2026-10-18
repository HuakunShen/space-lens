<script lang="ts" module>
  /**
   * The client-side Quit list (§6.2 item 6): groups the user staged from the
   * table, the drawer, the free-up dialog, or the AI advisor. Nothing here
   * executes — the page turns the list into a graceful plan when the user picks
   * "Review & quit…".
   */
  import type { TerminateOrigin } from './TerminateDialog.svelte'

  /** One staged Quit-list entry. Structurally identical to the page's model. */
  export interface QuitListEntry {
    /** Stable key: `group:<groupId>`. */
    key: string
    groupId: string
    name: string
    origin: TerminateOrigin
    /** The plain-language reason shown under the name, or `null`. */
    reason: string | null
    stagedAt: string
  }
</script>

<script lang="ts">
  /**
   * The staged Quit list, modelled on `CollectorPanel` (§6.2 item 6). Each row
   * carries an origin badge (You / Free up / AI) and the reason it was staged;
   * "Review & quit…" hands the request to the page, which builds a graceful plan
   * and opens the terminate dialog. Force is never offered here.
   *
   * Props only; no transport.
   */
  import { Minus } from '@lucide/svelte'
  import { Badge } from '../ui/badge/index'
  import { Button } from '../ui/button/index'
  import * as Sheet from '../ui/sheet/index'
  import { ScrollArea } from '../ui/scroll-area/index'
  import { ORIGIN_LABEL } from './TerminateDialog.svelte'

  interface Props {
    open: boolean
    entries: readonly QuitListEntry[]
    onClose: () => void
    onRemove: (key: string) => void
    /** The page builds a graceful plan for the staged groups and opens the dialog. */
    onReview: () => void
    onClear?: () => void
    planning?: boolean
    /** False when this host does not allow process control (§6.4). */
    terminateAvailable?: boolean
    class?: string
  }

  let {
    open,
    entries,
    onClose,
    onRemove,
    onReview,
    onClear,
    planning = false,
    terminateAvailable = true,
    class: className = '',
  }: Props = $props()
</script>

<Sheet.Root {open} onOpenChange={(value) => { if (!value && !planning) onClose() }}>
  <Sheet.Content class={`flex w-full flex-col gap-0 sm:max-w-xl ${className}`} showCloseButton={!planning}>
    <Sheet.Header class="border-b pb-4">
      <Sheet.Title>Quit list</Sheet.Title>
      <Sheet.Description>
        {entries.length} item{entries.length === 1 ? '' : 's'} staged · nothing is quit until you review and confirm.
      </Sheet.Description>
    </Sheet.Header>

    <ScrollArea class="min-h-0 flex-1">
      <div class="flex flex-col gap-3 px-4 py-4">
        {#if !terminateAvailable}
          <p class="rounded-md border p-3 text-sm">This host does not allow process control.</p>
        {/if}

        {#if entries.length === 0}
          <p class="text-sm text-muted-foreground">
            Choose apps from the table, the drawer, or the free-up dialog to stage them here.
          </p>
        {:else}
          <ul role="list" class="divide-y rounded-md border text-sm">
            {#each entries as entry (entry.key)}
              <li class="flex items-center gap-2 px-3 py-2">
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2">
                    <span class="truncate font-medium">{entry.name}</span>
                    <Badge variant="outline">{ORIGIN_LABEL[entry.origin]}</Badge>
                  </div>
                  {#if entry.reason}
                    <p class="truncate text-xs text-muted-foreground">{entry.reason}</p>
                  {/if}
                </div>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  type="button"
                  aria-label={`Remove ${entry.name}`}
                  onclick={() => onRemove(entry.key)}
                >
                  <Minus size={15} />
                </Button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    </ScrollArea>

    <Sheet.Footer class="border-t pt-4">
      <Button variant="outline" type="button" onclick={onClose} disabled={planning}>Back to workspace</Button>
      {#if onClear && entries.length > 0}
        <Button variant="ghost" type="button" onclick={onClear} disabled={planning}>Clear</Button>
      {/if}
      <Button
        type="button"
        disabled={entries.length === 0 || planning || !terminateAvailable}
        onclick={onReview}
      >
        {planning ? 'Planning…' : 'Review & quit…'}
      </Button>
    </Sheet.Footer>
  </Sheet.Content>
</Sheet.Root>
