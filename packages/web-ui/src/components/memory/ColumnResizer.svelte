<script lang="ts">
  /**
   * Drag handle on the right edge of a column header. Drag to resize, double-
   * click to restore the default, or focus it and use the arrow keys. The
   * caller owns the width; this only reports the new value.
   */
  import { clampWidth, type ColumnSpec } from '../../lib/memory/columns.ts'

  interface Props {
    label: string
    width: number
    spec: ColumnSpec
    onResize: (width: number) => void
    /** Called once a drag or key change settles, to persist the widths. */
    onCommit?: () => void
  }

  let { label, width, spec, onResize, onCommit }: Props = $props()

  let drag = $state<{ startX: number; startWidth: number } | null>(null)

  function onPointerDown(event: PointerEvent): void {
    if (event.button !== 0) return
    event.preventDefault()
    event.stopPropagation()
    ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
    drag = { startX: event.clientX, startWidth: width }
  }

  function onPointerMove(event: PointerEvent): void {
    if (!drag) return
    onResize(clampWidth(spec, drag.startWidth + event.clientX - drag.startX))
  }

  function onPointerUp(event: PointerEvent): void {
    if (!drag) return
    ;(event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId)
    drag = null
    onCommit?.()
  }

  function onKeydown(event: KeyboardEvent): void {
    const step = event.shiftKey ? 32 : 8
    if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
      event.preventDefault()
      event.stopPropagation()
      onResize(clampWidth(spec, width + (event.key === 'ArrowRight' ? step : -step)))
      onCommit?.()
    }
  }
</script>

<!-- A focusable separator is an ARIA widget (it has a value the keys change),
     which svelte's static check does not recognise. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  role="separator"
  aria-orientation="vertical"
  aria-label={`Resize ${label} column`}
  aria-valuenow={width}
  aria-valuemin={spec.min}
  aria-valuemax={spec.max}
  tabindex="0"
  class="column-resizer"
  data-dragging={drag !== null}
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  onpointercancel={onPointerUp}
  ondblclick={(event) => {
    event.stopPropagation()
    onResize(spec.defaultWidth)
    onCommit?.()
  }}
  onclick={(event) => event.stopPropagation()}
  onkeydown={onKeydown}
></div>

<style>
  .column-resizer {
    position: absolute;
    top: 0;
    right: -4px;
    z-index: 1;
    width: 8px;
    height: 100%;
    cursor: col-resize;
    touch-action: none;
  }
  .column-resizer::after {
    content: '';
    position: absolute;
    top: 25%;
    bottom: 25%;
    left: 3.5px;
    width: 1px;
    background: var(--border);
  }
  .column-resizer:hover::after,
  .column-resizer:focus-visible::after,
  .column-resizer[data-dragging='true']::after {
    top: 0;
    bottom: 0;
    width: 2px;
    left: 3px;
    background: var(--primary);
  }
  .column-resizer:focus-visible {
    outline: none;
  }
</style>
