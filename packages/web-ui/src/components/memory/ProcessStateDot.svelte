<script lang="ts">
  /**
   * The state of one process as a coloured dot, optionally with its label.
   * Colour is never the only signal: the tooltip and the optional label spell
   * the state out, and the table's State column / detail dialog repeat it.
   */
  import type { ProcessState } from '@space-lens/contract'
  import { PROCESS_STATE_DOT, PROCESS_STATE_LABELS, PROCESS_STATE_TEXT } from '../../lib/memory/labels.ts'

  interface Props {
    state: ProcessState
    showLabel?: boolean
    class?: string
  }

  let { state, showLabel = false, class: className = '' }: Props = $props()
  let label = $derived(PROCESS_STATE_LABELS[state])
</script>

<span class={`inline-flex min-w-0 items-center gap-1.5 ${className}`} title={label}>
  <span class={`inline-block size-2 shrink-0 rounded-full ring-2 ring-background/30 ${PROCESS_STATE_DOT[state]}`} aria-hidden="true"></span>
  {#if showLabel}<span class={`truncate ${PROCESS_STATE_TEXT[state]}`}>{label}</span>{/if}
  {#if !showLabel}<span class="sr-only">{label}</span>{/if}
</span>
