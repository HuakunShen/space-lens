<script lang="ts" module>
  /**
   * Wording for the terminate flow (T19; §6.2 item 6, §6.3 and §7 of the Memory
   * view plan), shared by the Quit list panel, the plan dialog
   * and the audit panel: the origin badges (You / Free up / AI), a signal method
   * in words, the short status chips, and one risk flag as a sentence. Keeping
   * them in the module context means the three components cannot drift.
   */
  import type {
    TerminatePlan,
    TerminatePlanEntry,
    TerminateRefusal,
    TerminateResult,
  } from '@space-lens/contract'

  // The contract ships the enums as literal arrays, so these unions are derived
  // from the wire types the same way ProcessTable derives `ProcessKind`.
  export type TerminateOrigin = TerminatePlan['origin']
  export type TerminateMethod = TerminatePlanEntry['method']
  export type TerminateStatus = TerminateResult['status']
  export type RiskFlag = TerminatePlanEntry['risks'][number]
  export type RefusalCode = TerminateRefusal['code']

  /** Origin badge text (§6.2 item 6: You / Free up / AI). */
  export const ORIGIN_LABEL: Record<TerminateOrigin, string> = {
    manual: 'You',
    freeUp: 'Free up',
    advisor: 'AI',
  }

  /** A signal method in words (§6.3). */
  export const METHOD_LABEL: Record<TerminateMethod, string> = {
    appQuit: 'Ask to quit',
    sigterm: 'Send quit signal',
    sigkill: 'Force quit',
  }

  /** Short status chip text for one executed entry (§6.3). */
  export const STATUS_LABEL: Record<TerminateStatus, string> = {
    exited: 'Quit',
    stillRunning: 'Still open',
    alreadyGone: 'Already gone',
    identityChanged: 'Changed since the plan',
    refused: 'Refused',
    failed: 'Failed',
  }

  /** Badge variant per status, so a failure reads as a failure in colour too. */
  export const STATUS_VARIANT: Record<TerminateStatus, 'secondary' | 'destructive' | 'outline'> = {
    exited: 'secondary',
    stillRunning: 'outline',
    alreadyGone: 'outline',
    identityChanged: 'outline',
    refused: 'outline',
    failed: 'destructive',
  }

  /** One risk flag as a sentence (§5 rule S21); every flag is shown, never hidden. */
  export function riskText(risk: RiskFlag, descendantCount: number | null): string {
    switch (risk) {
      case 'appMayPromptToSave':
        return 'The app may ask you to save changes before it quits.'
      case 'unsavedWorkMayBeLost':
        return 'Unsaved work in this app may be lost.'
      case 'endsChildProcesses':
        return descendantCount === null || descendantCount === 0
          ? "This ends the app's other processes too."
          : `This ends ${descendantCount} other processes in the group.`
      case 'backgroundAgent':
        return 'It runs in the background without a Dock icon.'
      case 'recentlyUsed':
        return 'You used it recently.'
      case 'restarts':
        return 'macOS may start it again after it quits.'
    }
  }
</script>

<script lang="ts">
  /**
   * The immutable terminate plan dialog (§6.3): every entry with its method in
   * words, the risk lines, the refused items with their reasons, the expected
   * release ("about 2.4 GB") and the countdown to the plan's `expiresAt`. A
   * force plan stays disabled until the extra acknowledgement checkbox is
   * ticked. After the host replies, each entry shows its status, and a
   * `stillRunning` entry offers a new force plan.
   *
   * Props only: the dialog never builds a plan or calls the transport.
   */
  import type { TerminateOutcome } from '@space-lens/contract'
  import { LoaderCircle } from '@lucide/svelte'
  import { Badge } from '../ui/badge/index'
  import { Button } from '../ui/button/index'
  import { Checkbox } from '../ui/checkbox/index'
  import * as Sheet from '../ui/sheet/index'
  import { ScrollArea } from '../ui/scroll-area/index'
  import { formatBytes } from '../../lib/memory/format.ts'
  import { protectionReason, type Protection } from './ProcessTable.svelte'

  interface Props {
    /** The plan to confirm, or `null` when the dialog is closed. */
    plan: TerminatePlan | null
    /** The host's reply once execute ran; switches the dialog to outcome mode. */
    outcome?: TerminateOutcome | null
    error?: string | null
    executing?: boolean
    /** Confirm the plan; the page sends `{planId, confirm: true}`. */
    onConfirm: (plan: TerminatePlan) => void
    /** Build a new force plan for a `stillRunning` entry (never automatic). */
    onForceQuit?: (result: TerminateResult) => void
    onClose: () => void
    /** Injectable clock for the countdown; defaults to `Date.now`. */
    now?: () => number
    /** Used memory from the snapshot before execute, for the measured line (§7). */
    measuredBeforeBytes?: number | null
    /** Used memory from the snapshot after execute, for the measured line (§7). */
    measuredAfterBytes?: number | null
    /** Seconds between the before and after snapshots, for the measured line (§7). */
    measuredSeconds?: number | null
    class?: string
  }

  let {
    plan,
    outcome = null,
    error = null,
    executing = false,
    onConfirm,
    onForceQuit,
    onClose,
    now = () => Date.now(),
    measuredBeforeBytes = null,
    measuredAfterBytes = null,
    measuredSeconds = null,
    class: className = '',
  }: Props = $props()

  let measured = $derived(
    measuredBeforeBytes !== null && measuredAfterBytes !== null && measuredSeconds !== null,
  )

  let acknowledged = $state(false)
  let remainingMs = $state(0)

  // Start the countdown from `expiresAt` as soon as a plan appears, then walk it
  // down once a second. The clock is a prop so tests and the page can inject one.
  $effect(() => {
    const current = plan
    if (!current) {
      remainingMs = 0
      return
    }
    const update = (): void => {
      remainingMs = Date.parse(current.expiresAt) - now()
    }
    update()
    const handle = setInterval(update, 1000)
    return () => clearInterval(handle)
  })

  // A fresh plan starts unacknowledged.
  $effect(() => {
    void plan?.planId
    acknowledged = false
  })

  let expired = $derived(plan !== null && remainingMs <= 0)
  let remainingSeconds = $derived(Math.max(0, Math.ceil(remainingMs / 1000)))

  let entries = $derived(plan?.entries ?? [])
  let quitCount = $derived(new Set(entries.map((entry) => entry.groupId)).size)
  let canConfirm = $derived(
    plan !== null &&
      !executing &&
      !expired &&
      entries.length > 0 &&
      (plan.mode !== 'force' || acknowledged),
  )

  /** A refusal code as a sentence, reusing the protection reasons from ProcessTable (§5). */
  function refusalText(code: RefusalCode, protection: Protection | null): string {
    switch (code) {
      case 'protected':
        return protectionReason(protection) ?? 'This process is protected.'
      case 'notFound':
        return 'It is no longer running.'
      case 'identityChanged':
        return 'It changed since the plan was made.'
      case 'duplicate':
        return 'It was listed more than once, so only the first entry was kept.'
      case 'tooMany':
        return 'The plan had too many targets.'
      case 'frontmost':
        return 'It is the app you are using right now.'
    }
  }
</script>

<Sheet.Root open={plan !== null} onOpenChange={(open) => { if (!open && !executing) onClose() }}>
  <Sheet.Content class={`flex w-full flex-col gap-0 sm:max-w-xl ${className}`} showCloseButton={!executing}>
    {#if plan}
      <Sheet.Header class="border-b pb-4">
        <Sheet.Title>
          {plan.mode === 'force' ? 'Force quit' : 'Quit'}
          {quitCount}
          {quitCount === 1 ? 'app' : 'apps'}
        </Sheet.Title>
        <Sheet.Description>
          {ORIGIN_LABEL[plan.origin]} · about {formatBytes(plan.expectedReleaseBytes)} back if they all quit.
        </Sheet.Description>
      </Sheet.Header>

      <ScrollArea class="min-h-0 flex-1">
        <div class="flex flex-col gap-4 px-4 py-4">
          {#if error}
            <p class="text-sm text-destructive" role="alert">{error}</p>
          {/if}

          {#if outcome}
            <div class="flex flex-col gap-3">
              {#if measured}
                <p class="text-sm font-medium">
                  Released about {formatBytes(outcome.releasedBytesEstimate)}. Used memory went from {formatBytes(measuredBeforeBytes ?? 0)} to {formatBytes(measuredAfterBytes ?? 0)} over the next {measuredSeconds ?? 0} s.
                </p>
              {:else}
                <p class="text-sm font-medium">
                  {outcome.results.length} processes handled · about
                  {formatBytes(outcome.releasedBytesEstimate)} released.
                </p>
              {/if}
              <ul role="list" class="divide-y rounded-md border text-sm">
                {#each outcome.results as result (result.pid)}
                  <li class="flex flex-col gap-1 px-3 py-2">
                    <div class="flex items-center justify-between gap-2">
                      <span class="truncate font-medium">{result.name}</span>
                      <Badge variant={STATUS_VARIANT[result.status]}>{STATUS_LABEL[result.status]}</Badge>
                    </div>
                    <p class="text-xs text-muted-foreground">{METHOD_LABEL[result.method]}</p>
                    {#if result.status === 'stillRunning'}
                      <p class="text-xs text-muted-foreground">
                        Still open — it may be asking you to save changes. Check its window, or force quit.
                      </p>
                      {#if onForceQuit}
                        <div>
                          <Button variant="destructive" size="xs" type="button" onclick={() => onForceQuit?.(result)}>
                            Force quit…
                          </Button>
                        </div>
                      {/if}
                    {/if}
                    {#if result.errno !== null}
                      <p class="text-xs text-muted-foreground">The system reported error {result.errno}.</p>
                    {/if}
                  </li>
                {/each}
              </ul>
            </div>
          {:else}
            {#if expired}
              <p class="text-sm text-destructive" role="alert">
                This plan has expired. Close it and review the quit list again.
              </p>
            {:else}
              <p class="text-xs text-muted-foreground">This plan expires in {remainingSeconds} s.</p>
            {/if}

            <ul role="list" class="flex flex-col gap-2">
              {#each entries as entry (entry.pid)}
                <li class="rounded-md border p-3">
                  <div class="flex items-center justify-between gap-2">
                    <span class="truncate font-medium">{entry.name}</span>
                    <span class="shrink-0 text-xs text-muted-foreground">{METHOD_LABEL[entry.method]}</span>
                  </div>
                  <div class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
                    <span>PID {entry.pid}</span>
                    <span>
                      {entry.footprintBytes === null ? 'Memory not readable' : `${formatBytes(entry.footprintBytes)} in use`}
                    </span>
                  </div>
                  {#if entry.risks.length > 0}
                    <ul role="list" class="mt-2 list-disc space-y-0.5 pl-5 text-xs">
                      {#each entry.risks as risk}
                        <li>{riskText(risk, entry.descendantCount)}</li>
                      {/each}
                    </ul>
                  {/if}
                </li>
              {/each}
            </ul>

            {#if plan.refused.length > 0}
              <div>
                <p class="mb-1 text-xs font-medium tracking-wide text-muted-foreground uppercase">
                  Left out of this plan
                </p>
                <ul role="list" class="divide-y rounded-md border text-sm">
                  {#each plan.refused as refusal, index (refusal.pid ?? refusal.groupId ?? index)}
                    <li class="flex items-center justify-between gap-2 px-2 py-1.5">
                      <span class="truncate">{refusal.name}</span>
                      <span class="shrink-0 text-xs text-muted-foreground">
                        {refusalText(refusal.code, refusal.protection)}
                      </span>
                    </li>
                  {/each}
                </ul>
              </div>
            {/if}

            {#if plan.mode === 'force'}
              <label class="flex items-start gap-2 rounded-md border border-destructive/40 p-3 text-sm">
                <Checkbox
                  checked={acknowledged}
                  onCheckedChange={(value) => (acknowledged = value === true)}
                  aria-label="I understand unsaved work in these apps will be lost"
                />
                <span>I understand unsaved work in these apps will be lost</span>
              </label>
            {/if}
          {/if}
        </div>
      </ScrollArea>

      <Sheet.Footer class="border-t pt-4">
        <Button variant="outline" type="button" onclick={onClose} disabled={executing}>Close</Button>
        {#if !outcome}
          <Button
            variant={plan.mode === 'force' ? 'destructive' : 'default'}
            type="button"
            disabled={!canConfirm}
            onclick={() => {
              if (plan) onConfirm(plan)
            }}
          >
            {#if executing}<LoaderCircle size={16} class="animate-spin" />{/if}
            {plan.mode === 'force' ? `Force quit ${quitCount}` : `Quit ${quitCount}`}
            {quitCount === 1 ? 'app' : 'apps'}
          </Button>
        {/if}
      </Sheet.Footer>
    {/if}
  </Sheet.Content>
</Sheet.Root>
