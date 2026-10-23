<script lang="ts">
  /**
   * The "Free up memory" dialog (§6.2 item 1 and §7 of the Memory view plan):
   * the pressure headline in words, the candidate
   * apps with their reason lines and risk lines, checkboxes with the
   * preselected candidates ticked, the always-on footer that says plainly what
   * Space Lens can and cannot do, and a Review button that hands the chosen
   * group ids to the page.
   *
   * Force is never offered from this flow; a `stillRunning` row in the terminate
   * dialog is the only place a force plan appears. Props only; no transport.
   */
  import type { FreeUpCandidates } from '@space-lens/contract'
  import { LoaderCircle } from '@lucide/svelte'
  import { Button } from '../ui/button/index'
  import { Checkbox } from '../ui/checkbox/index'
  import * as Sheet from '../ui/sheet/index'
  import { ScrollArea } from '../ui/scroll-area/index'
  import { formatBytes } from '../../lib/memory/format.ts'
  import { pressureHeadline, reasonText } from '../../lib/memory/reasons.ts'
  import { riskText } from './TerminateDialog.svelte'

  interface Props {
    open: boolean
    /** The latest candidates reply, or `null` before the first load. */
    candidates: FreeUpCandidates | null
    /** Review the chosen group ids as a graceful `freeUp` plan. */
    onReview: (groupIds: string[]) => void
    onClose: () => void
    /** Add a group to the caller's never-suggest list (§7 `neverSuggest`). */
    onNeverSuggest?: (groupId: string) => void
    planning?: boolean
    class?: string
  }

  let {
    open,
    candidates,
    onReview,
    onClose,
    onNeverSuggest,
    planning = false,
    class: className = '',
  }: Props = $props()

  /**
   * The always-on footer (§7). It is the one sanctioned place a banned word
   * appears: the sentence explicitly says Space Lens does not run such a trick.
   * The banned-words test removes this exact phrase before scanning.
   */
  const FREE_UP_FOOTER =
    "Space Lens can only quit apps. It does not run 'memory purge' tricks: they briefly raise free memory by pushing other apps into compression or swap, which makes your Mac slower."

  let selection = $state<Set<string>>(new Set())
  let lastSequence = $state(-1)

  // Preselect the candidates the algorithm marked as idle apps; re-seed only
  // when a new candidates reply arrives (new sequence), never on every render.
  $effect(() => {
    if (!candidates) {
      selection = new Set()
      lastSequence = -1
      return
    }
    if (candidates.sequence === lastSequence) return
    lastSequence = candidates.sequence
    selection = new Set(
      candidates.candidates.filter((candidate) => candidate.preselected).map((candidate) => candidate.groupId),
    )
  })

  let headline = $derived(
    candidates === null
      ? ''
      : pressureHeadline(candidates.pressure.level, candidates.potentialReleaseBytes),
  )

  let noCandidatesText = $derived.by(() => {
    const minutes = candidates?.observedMinutes ?? 0
    const watched = minutes > 0 ? ` while this view was open (${minutes} min)` : ''
    return `No idle large apps found${watched}. Keep this view open a few minutes for better suggestions, or pick apps yourself in the table.`
  })

  let selectedIds = $derived(
    candidates === null
      ? []
      : candidates.candidates
          .filter((candidate) => selection.has(candidate.groupId))
          .map((candidate) => candidate.groupId),
  )
  let canReview = $derived(
    candidates !== null && candidates.candidates.length > 0 && selectedIds.length > 0 && !planning,
  )

  function toggle(groupId: string): void {
    const next = new Set(selection)
    if (next.has(groupId)) next.delete(groupId)
    else next.add(groupId)
    selection = next
  }
</script>

<Sheet.Root {open} onOpenChange={(value) => { if (!value && !planning) onClose() }}>
  <Sheet.Content class={`flex w-full flex-col gap-0 sm:max-w-xl ${className}`} showCloseButton={!planning}>
    <Sheet.Header class="border-b pb-4">
      <Sheet.Title>Free up memory</Sheet.Title>
      <Sheet.Description>
        {#if candidates}
          {candidates.observedMinutes} min observed · {candidates.candidates.length} suggestion{candidates.candidates.length === 1 ? '' : 's'} · about
          {formatBytes(candidates.potentialReleaseBytes)} back.
        {:else}
          Looking for idle, large apps…
        {/if}
      </Sheet.Description>
    </Sheet.Header>

    <ScrollArea class="min-h-0 flex-1">
      <div class="flex flex-col gap-4 px-4 py-4">
        {#if candidates}
          <p class="text-sm">{headline}</p>

          {#if candidates.candidates.length === 0}
            <p class="rounded-md border p-3 text-sm text-muted-foreground">{noCandidatesText}</p>
          {:else}
            <ul role="list" class="flex flex-col gap-2">
              {#each candidates.candidates as candidate (candidate.groupId)}
                <li class="rounded-md border p-3">
                  <label class="flex items-start gap-2">
                    <Checkbox
                      checked={selection.has(candidate.groupId)}
                      onCheckedChange={() => toggle(candidate.groupId)}
                      aria-label={`Select ${candidate.name}`}
                    />
                    <span class="flex min-w-0 flex-1 items-center justify-between gap-2">
                      <span class="truncate font-medium">{candidate.name}</span>
                      <span class="shrink-0 text-xs tabular-nums text-muted-foreground">
                        {formatBytes(candidate.footprintBytes)}
                      </span>
                    </span>
                  </label>

                  {#if candidate.reasons.length > 0}
                    <ul role="list" class="mt-2 list-disc space-y-0.5 pl-6 text-xs text-muted-foreground">
                      {#each candidate.reasons as reason}
                        <li>{reasonText(reason)}</li>
                      {/each}
                    </ul>
                  {/if}

                  {#if candidate.risks.length > 0}
                    <ul role="list" class="mt-1 space-y-0.5 pl-0 text-xs text-amber-600 dark:text-amber-400">
                      {#each candidate.risks as risk}
                        <li>{riskText(risk, null)}</li>
                      {/each}
                    </ul>
                  {/if}

                  {#if onNeverSuggest}
                    <div class="mt-2">
                      <Button
                        variant="ghost"
                        size="xs"
                        type="button"
                        onclick={() => onNeverSuggest?.(candidate.groupId)}
                      >
                        Never suggest
                      </Button>
                    </div>
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}

          <p class="rounded-md bg-muted p-3 text-xs text-muted-foreground">{FREE_UP_FOOTER}</p>
        {/if}
      </div>
    </ScrollArea>

    <Sheet.Footer class="border-t pt-4">
      <Button variant="outline" type="button" onclick={onClose} disabled={planning}>Back to workspace</Button>
      <Button type="button" disabled={!canReview} onclick={() => onReview(selectedIds)}>
        {#if planning}<LoaderCircle size={16} class="animate-spin" />{/if}
        {planning ? 'Planning…' : 'Review'}
      </Button>
    </Sheet.Footer>
  </Sheet.Content>
</Sheet.Root>
