<script lang="ts">
  /**
   * One card in the process-relationship diagram: the process name, the memory
   * it costs and how much of the focused branch that is.
   *
   * The role decides the styling: the focused process is ringed, its ancestors
   * are dashed, and the siblings drawn for context are muted. The card is
   * exactly `GRAPH_NODE_WIDTH` wide — the layout in `lib/memory/graph.ts`
   * assumes it.
   */
  import { Handle, Position, type NodeProps } from '@xyflow/svelte'
  import { Lock } from '@lucide/svelte'
  import { formatBytes } from '../../lib/memory/format.ts'
  import type { ProcessFlowNode } from '../../lib/memory/flow.ts'
  import { GRAPH_NODE_WIDTH } from '../../lib/memory/graph.ts'
  import KindGlyph from './KindGlyph.svelte'

  let { data }: NodeProps<ProcessFlowNode> = $props()

  let node = $derived(data.node)
  let focused = $derived(node.role === 'focus')
  let context = $derived(node.role === 'sibling')
  let percent = $derived(Math.round(node.share * 100))
  let cardClass = $derived(
    [
      'process-graph-card relative rounded-lg border px-2.5 py-2 shadow-sm transition-colors',
      context ? 'bg-muted/40' : 'bg-card',
      node.role === 'ancestor' || context ? 'border-dashed' : '',
      focused ? 'border-primary ring-2 ring-primary/25' : '',
      context ? 'opacity-70' : '',
    ].join(' '),
  )
</script>

<div
  class={cardClass}
  style:width={`${GRAPH_NODE_WIDTH}px`}
  title={`${node.name} · pid ${node.pid}${node.groupName === null ? '' : ` · ${node.groupName}`}`}
  data-role={node.role}
  data-pid={node.pid}
>
  <Handle type="target" position={Position.Top} isConnectable={false} />

  <div class="flex items-center gap-1.5">
    <KindGlyph kind={node.kind} class="size-3.5 shrink-0 text-muted-foreground" />
    <span class="min-w-0 flex-1 truncate text-xs font-medium" class:text-foreground={focused}>{node.name}</span>
    {#if node.restricted}
      <Lock class="size-3 shrink-0 text-muted-foreground" aria-label="Details hidden" />
    {/if}
  </div>

  <div class="mt-1 flex items-baseline justify-between gap-2 text-[10px] tabular-nums text-muted-foreground">
    <span class="font-medium text-foreground">
      {node.ownBytes === null ? '—' : formatBytes(node.ownBytes)}
    </span>
    {#if node.hiddenChildren > 0}
      <span title={`${node.hiddenChildren} more ${node.hiddenChildren === 1 ? 'child' : 'children'} not drawn`}>
        +{node.hiddenChildren}
      </span>
    {:else if node.descendantCount > 0}
      <span>{node.descendantCount} {node.descendantCount === 1 ? 'proc' : 'procs'}</span>
    {:else if node.cpuPercent !== null}
      <span>{node.cpuPercent.toFixed(0)}% cpu</span>
    {:else}
      <span>pid {node.pid}</span>
    {/if}
  </div>

  <div class="mt-1.5 h-0.5 overflow-hidden rounded-full bg-border/60" aria-hidden="true">
    <div class="h-full rounded-full bg-primary/60" style:width={`${percent}%`}></div>
  </div>

  <Handle type="source" position={Position.Bottom} isConnectable={false} />
</div>
