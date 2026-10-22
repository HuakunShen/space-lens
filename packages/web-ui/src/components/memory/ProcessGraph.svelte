<script lang="ts">
  /**
   * The right-hand detail view of a process: the ancestor chain above it, the
   * process itself and its subtree below, drawn as a node-link diagram.
   *
   * The heavy lifting is in `lib/memory/graph.ts` — this component only maps
   * `GraphNode`s to Svelte Flow nodes and paints them. Clicking a card moves
   * the focus to that process, so the diagram can be walked up and down like a
   * file explorer; the tree on the left stays the flat list.
   *
   * A process tree does not fit a detail pane at a readable size, so the view
   * opens at a fixed, legible zoom centred on the chain and the rest is panned;
   * the mini map in the corner keeps the shape of the whole tree in sight.
   *
   * Props only; the page owns the transport and the focus state.
   */
  import {
    SvelteFlow,
    Background,
    Controls,
    MiniMap,
    type Edge,
    type Node,
  } from '@xyflow/svelte'
  import '@xyflow/svelte/dist/style.css'
  import type { MemorySnapshot } from '@space-lens/contract'
  import { X } from '@lucide/svelte'
  import { Button } from '../ui/button/index'
  import { formatBytes } from '../../lib/memory/format.ts'
  import { buildForest } from '../../lib/memory/tree.ts'
  import {
    GRAPH_NODE_WIDTH,
    buildProcessGraph,
    type GraphEdgeKind,
    type GraphNode,
  } from '../../lib/memory/graph.ts'
  import type { ProcessFlowNode } from '../../lib/memory/flow.ts'
  import KindGlyph from './KindGlyph.svelte'
  import ProcessGraphNode from './ProcessGraphNode.svelte'

  interface Props {
    snapshot: MemorySnapshot
    /** The process to draw the neighbourhood of. */
    pid: number
    /** Move the focus to another process (a card click). */
    onFocus?: (pid: number) => void
    /** Close the panel. */
    onClose?: () => void
    class?: string
  }

  let { snapshot, pid, onFocus, onClose, class: className = '' }: Props = $props()

  /** The zoom the panel opens at: below ~0.75 the card text stops reading. */
  const VIEW_ZOOM = 0.82
  /** Card height used to centre the chain; the card itself is 58–66 px. */
  const CARD_CENTER = 30

  const nodeTypes = { process: ProcessGraphNode }

  const EDGE_STYLE: Record<GraphEdgeKind, string> = {
    // The path from the topmost ancestor down to the focus.
    chain: 'stroke: var(--primary); stroke-width: 1.5; opacity: 0.45',
    // The focus's own subtree.
    branch: 'stroke: var(--border); stroke-width: 1.5',
    // Siblings, drawn only for context.
    context: 'stroke: var(--border); stroke-width: 1; stroke-dasharray: 4 4; opacity: 0.6',
  }

  let dark = $state(false)
  let paneWidth = $state(0)
  let paneHeight = $state(0)

  $effect(() => {
    const root = document.documentElement
    const update = () => (dark = root.classList.contains('dark'))
    update()
    const observer = new MutationObserver(update)
    observer.observe(root, { attributes: true, attributeFilter: ['class'] })
    return () => observer.disconnect()
  })

  let forest = $derived(buildForest(snapshot))
  let graph = $derived(buildProcessGraph(forest, pid))

  /** The chain from the topmost ancestor to the focus, for the header. */
  let chain = $derived(
    graph === null
      ? []
      : graph.nodes.filter((node) => node.role === 'ancestor' || node.role === 'focus'),
  )
  let chainText = $derived(chain.map((node) => node.name).join(' → '))
  let parentText = $derived(
    chain.length > 1 ? chain.slice(0, -1).map((node) => node.name).join(' → ') : null,
  )

  let flowNodes = $derived<ProcessFlowNode[]>(
    (graph?.nodes ?? []).map((node) => ({
      id: String(node.pid),
      type: 'process',
      position: { x: node.x - GRAPH_NODE_WIDTH / 2, y: node.y },
      data: { node },
      draggable: false,
      selectable: false,
    })),
  )

  let flowEdges = $derived<Edge[]>(
    (graph?.edges ?? []).map((edge) => ({
      id: `${edge.source}-${edge.target}`,
      source: String(edge.source),
      target: String(edge.target),
      type: 'smoothstep',
      style: EDGE_STYLE[edge.kind],
    })),
  )

  /**
   * Where the diagram opens. The focused card is at x = 0, so half the pane
   * puts it in the middle. Vertically a tree that fits is centred whole; when
   * the chain is taller than the pane the focused card stays in the lower
   * third with its children under it, and the ancestors run off the top edge to
   * be panned into view — never the other way round, because the focused
   * process must always be visible.
   */
  let viewport = $derived.by(() => {
    if (graph === null || paneWidth === 0 || paneHeight === 0) {
      return { x: 0, y: 0, zoom: VIEW_ZOOM }
    }
    const focusCenter = graph.focus.y + CARD_CENTER
    const minY = Math.min(...graph.nodes.map((node) => node.y))
    const maxY = Math.max(...graph.nodes.map((node) => node.y)) + CARD_CENTER * 2
    const contentHeight = (maxY - minY) * VIEW_ZOOM
    const y =
      contentHeight <= paneHeight - 16
        ? (paneHeight - contentHeight) / 2 - minY * VIEW_ZOOM
        : paneHeight * 0.62 - focusCenter * VIEW_ZOOM
    return { x: paneWidth / 2, y, zoom: VIEW_ZOOM }
  })

  function minimapColor(node: Node): string {
    switch ((node.data as { node?: GraphNode }).node?.role) {
      case 'focus':
        return 'var(--primary)'
      case 'ancestor':
        return 'var(--muted-foreground)'
      case 'sibling':
        return 'var(--border)'
      default:
        return 'var(--chart-3)'
    }
  }

  function memoryText(node: GraphNode): string {
    return node.ownBytes === null ? '—' : formatBytes(node.ownBytes)
  }
</script>

<section
  class={`process-graph flex flex-col overflow-hidden rounded-lg border bg-card text-card-foreground shadow-sm ${className}`}
  aria-label="Process relationships"
>
  {#if graph === null}
    <header class="flex items-start gap-2 border-b px-3 py-2.5">
      <div class="min-w-0 flex-1">
        <h2 class="text-sm font-semibold">Process not in this snapshot</h2>
        <p class="mt-0.5 text-[11px] text-muted-foreground">
          The process exited, or the snapshot no longer lists it.
        </p>
      </div>
      {#if onClose}
        <Button variant="ghost" size="xs" aria-label="Close the process diagram" onclick={onClose}>
          <X class="size-3.5" />
        </Button>
      {/if}
    </header>
    <div class="flex h-[24rem] items-center justify-center text-sm text-muted-foreground">
      No relationship to draw.
    </div>
  {:else}
    <header class="flex items-start gap-2.5 border-b px-3 py-2.5">
      <KindGlyph kind={graph.focus.kind} class="mt-0.5 size-4 shrink-0 text-muted-foreground" />
      <div class="min-w-0 flex-1">
        <div class="flex items-baseline gap-2">
          <h2 class="truncate text-sm font-semibold" title={graph.focus.name}>{graph.focus.name}</h2>
          <span class="shrink-0 text-xs tabular-nums text-muted-foreground">
            {formatBytes(graph.focus.subtreeBytes)}{#if graph.focus.descendantCount > 0}{' · '}{graph.focus.descendantCount}
            {graph.focus.descendantCount === 1 ? 'proc' : 'procs'}{/if}
          </span>
        </div>
        <div class="mt-0.5 flex items-baseline gap-2 text-[11px] text-muted-foreground">
          <span class="min-w-0 flex-1 truncate" title={chainText}>
            {#if parentText === null}
              No visible parent in this snapshot
            {:else}
              Launched by {parentText}
            {/if}
          </span>
          <span class="shrink-0 text-muted-foreground/70 tabular-nums">
            pid {graph.focus.pid} · self {memoryText(graph.focus)}
          </span>
        </div>
      </div>
      {#if onClose}
        <Button variant="ghost" size="xs" aria-label="Close the process diagram" onclick={onClose}>
          <X class="size-3.5" />
        </Button>
      {/if}
    </header>

    <div
      class="relative h-[clamp(24rem,60vh,42rem)] w-full"
      bind:clientWidth={paneWidth}
      bind:clientHeight={paneHeight}
    >
      {#if paneWidth > 0 && paneHeight > 0}
        {#key pid}
          <SvelteFlow
            nodes={flowNodes}
            edges={flowEdges}
            {nodeTypes}
            initialViewport={viewport}
            nodesDraggable={false}
            nodesConnectable={false}
            elementsSelectable={false}
            zoomOnDoubleClick={false}
            minZoom={0.2}
            maxZoom={1.6}
            colorMode={dark ? 'dark' : 'light'}
            onnodeclick={({ node }) => onFocus?.(Number(node.id))}
            class="process-graph-flow"
          >
            <Background gap={20} size={1} />
            <MiniMap
              position="top-right"
              width={132}
              height={86}
              pannable
              zoomable
              nodeBorderRadius={2}
              nodeColor={minimapColor}
              maskColor="color-mix(in oklab, var(--background) 72%, transparent)"
            />
            <Controls position="bottom-right" showZoom showFitView showLock={false} />
          </SvelteFlow>
        {/key}
      {/if}
      {#if graph.hiddenAncestors > 0}
        <p
          class="pointer-events-none absolute left-2 top-2 rounded-md border bg-background/85 px-2 py-1 text-[10px] text-muted-foreground"
        >
          {graph.hiddenAncestors} more {graph.hiddenAncestors === 1 ? 'ancestor' : 'ancestors'} above
        </p>
      {/if}
    </div>

    <footer
      class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 border-t px-3 py-2 text-[11px] text-muted-foreground"
    >
      <span>Click a card to make it the focus · drag to pan, scroll to zoom</span>
      {#if graph.hiddenProcesses > 0}
        <span>{graph.hiddenProcesses} below not drawn</span>
      {:else}
        <span>{graph.nodes.length} cards</span>
      {/if}
    </footer>
  {/if}
</section>

<style>
  /* Svelte Flow ships its own palette; point it at the app's tokens instead so
     the diagram matches the surrounding cards in both themes. */
  .process-graph :global(.svelte-flow) {
    --xy-background-color: var(--card);
    --xy-background-pattern-color: var(--border);
    --xy-edge-stroke: var(--border);
    --xy-edge-stroke-default: var(--border);
    --xy-controls-button-background-color: var(--background);
    --xy-controls-button-background-color-hover: var(--accent);
    --xy-controls-button-color: var(--foreground);
    --xy-controls-button-border-color: var(--border);
    --xy-controls-box-shadow: 0 0 0 1px var(--border);
    --xy-attribution-background-color: transparent;
    --xy-minimap-background-color: var(--background);
    --xy-minimap-mask-background-color: color-mix(in oklab, var(--background) 70%, transparent);
  }

  /* Edges are anchored by handles, but the diagram is not connectable. */
  .process-graph :global(.svelte-flow__handle) {
    opacity: 0;
    pointer-events: none;
    min-width: 0;
    min-height: 0;
  }

  .process-graph :global(.svelte-flow__node) {
    cursor: pointer;
  }

  /* The mini map is an overview, not a second diagram. */
  .process-graph :global(.svelte-flow__minimap) {
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    opacity: 0.85;
  }

  .process-graph :global(.svelte-flow__attribution) {
    opacity: 0.35;
    font-size: 9px;
  }
</style>
