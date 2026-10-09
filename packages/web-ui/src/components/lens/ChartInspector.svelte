<script lang="ts">
  import { Folder, File } from '@lucide/svelte'
  import type { TreeNodeSummary } from '../../types'
  import { formatNodeSize } from '../../lib/node-size'

  /** The part of a painted primitive the inspector needs; every chart's
   * primitive type satisfies this shape, so one inspector serves them all. */
  interface Highlight {
    color: string
    isAggregate: boolean
    childCount: number
  }

  interface Props {
    /** What the inspector describes: the hovered node, or the focused one. */
    inspected: TreeNodeSummary | null
    active?: Highlight | undefined
    hoveredId: string | null
    focusNode: TreeNodeSummary | null
    percentageLabel: string
    /** What this chart calls one of its primitives, for the empty-state hint. */
    noun: string
  }

  let { inspected, active, hoveredId, focusNode, percentageLabel, noun }: Props = $props()
</script>

<div
  class="chart-inspector glass-panel macos:rounded-md macos:border-0 macos:border-t macos:bg-transparent macos:px-1 macos:pt-3 windows:rounded-lg windows:bg-card linux:rounded-xl linux:bg-card linux:p-4"
  class:inspecting={hoveredId !== null && inspected !== focusNode}
  aria-live="polite"
  aria-atomic="true"
>
  <div
    class="inspector-icon macos:border-0 macos:bg-transparent macos:p-1 linux:rounded-lg"
    style={`--node: ${active?.color ?? 'var(--muted-foreground)'}`}
  >
    {#if inspected?.hasChildren || inspected?.collapsed}<Folder size={19} />{:else}<File size={19} />{/if}
  </div>
  <div class="inspector-content">
    <div class="inspector-heading">
      <strong>{inspected?.name ?? 'Explore your storage'}</strong><span>{formatNodeSize(inspected)}</span>
    </div>
    <p class="inspector-path">{inspected?.path ?? `Hover or focus a ${noun} to see its full path.`}</p>
    <p class="inspector-hint">
      {#if inspected?.scanState === 'skipped'}Not scanned · {inspected.skipReason ??
          'unavailable'}{:else if active?.isAggregate}{active.childCount.toLocaleString()} smaller items grouped here ·
        open the parent folder to explore{:else if hoveredId && inspected}{percentageLabel} of this folder · {inspected.hasChildren
          ? 'Click to explore'
          : 'Click to inspect'}{:else}Hover to inspect · click to explore{/if}
    </p>
  </div>
</div>
