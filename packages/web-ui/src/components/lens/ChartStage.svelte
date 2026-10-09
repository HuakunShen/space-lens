<script lang="ts">
  import type { Snippet } from 'svelte'
  import { ArrowUpLeft } from '@lucide/svelte'
  import type { ChartSize } from '../../lib/chart-size'

  interface Props {
    /** Accessible name for the canvas group, e.g. "Icicle disk usage chart". */
    label: string
    /** Message shown over the canvas when there is nothing to paint. */
    empty: string | null
    canGoBack?: boolean
    onBack?: () => void
    /** Extra class on the stage, for charts that lay out in HTML, not SVG. */
    class?: string
    /**
     * The measured content box, written back to the owning chart. Charts keep
     * this in their own state so their geometry can stay in `$derived` at the
     * top level instead of being rebuilt inside a snippet.
     */
    size?: ChartSize
    children?: Snippet
  }

  let {
    label,
    empty,
    canGoBack = false,
    onBack,
    class: className,
    size = $bindable({ width: 620, height: 430 }),
    children,
  }: Props = $props()

  let stage = $state<HTMLElement | null>(null)

  // The stage is the authority on how much room a chart has. Rounded to whole
  // pixels so a drag-resize does not rebuild geometry on every sub-pixel step.
  $effect(() => {
    if (!stage) return
    const observer = new ResizeObserver((entries) => {
      const box = entries[0]?.contentRect
      if (!box || box.width < 1 || box.height < 1) return
      const width = Math.round(box.width)
      const height = Math.round(box.height)
      if (width !== size.width || height !== size.height) size = { width, height }
    })
    observer.observe(stage)
    return () => observer.disconnect()
  })
</script>

<div
  class={`chart-stage macos:md:flex-1 windows:md:flex-1 linux:md:flex-1 ${className ?? ''}`}
  role="group"
  aria-label={label}
  bind:this={stage}
>
  {@render children?.()}
  {#if empty !== null}
    <div class="chart-empty">{empty}</div>
  {/if}
  {#if canGoBack}
    <button class="chart-back" type="button" onclick={onBack} aria-label="Go to parent folder"
      ><ArrowUpLeft size={14} /> Up one level</button
    >
  {/if}
</div>
