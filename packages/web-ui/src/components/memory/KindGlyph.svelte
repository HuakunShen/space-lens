<script lang="ts">
  /**
   * The glyph that stands for a process kind in the Memory view (§6.2). Shared
   * by the table and the tree so both views read the same way.
   */
  import { AppWindow, Bot, Boxes, Server, Square, Terminal } from '@lucide/svelte'
  import type { ProcessGroup } from '@space-lens/contract'

  /** `ProcessGroup['kind']` — the contract ships the enum as a literal union. */
  export type ProcessKind = ProcessGroup['kind']

  interface Props {
    kind: ProcessKind
    class?: string
  }

  let { kind, class: className = 'size-3.5 shrink-0 text-muted-foreground' }: Props = $props()
</script>

{#if kind === 'app'}
  <AppWindow class={className} />
{:else if kind === 'helper'}
  <Boxes class={className} />
{:else if kind === 'agent'}
  <Bot class={className} />
{:else if kind === 'cli'}
  <Terminal class={className} />
{:else if kind === 'system'}
  <Server class={className} />
{:else}
  <Square class={className} />
{/if}
