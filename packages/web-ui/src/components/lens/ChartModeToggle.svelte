<script lang="ts">
  import { ChartPie, LayoutGrid, Columns3, Circle, AlignJustify } from '@lucide/svelte'
  import type { ChartMode, TreemapDensity } from '../../lib/chart-mode'

  interface Props {
    mode: ChartMode
    onModeChange: (mode: ChartMode) => void
    /** The treemap's second axis; only meaningful while the treemap is active. */
    density: TreemapDensity
    onDensityChange: (density: TreemapDensity) => void
  }

  let { mode, onModeChange, density, onDensityChange }: Props = $props()

  const families = [
    { value: 'sunburst', label: 'Sunburst', hint: 'Rings by depth — the shape of a deep tree', icon: ChartPie },
    { value: 'treemap', label: 'Treemap', hint: 'Tiles by area — compare sibling sizes', icon: LayoutGrid },
    { value: 'icicle', label: 'Icicle', hint: 'Columns per level — read a path downward', icon: Columns3 },
    { value: 'bubbles', label: 'Bubbles', hint: 'Packed circles by area — a visual overview', icon: Circle },
    { value: 'strips', label: 'Strips', hint: 'Ranked rows — folders and what is inside them', icon: AlignJustify },
  ] as const

  const densities: ReadonlyArray<{ value: TreemapDensity; label: string; hint: string }> = [
    { value: 'flat', label: 'Flat', hint: 'One layer of tiles' },
    { value: 'nested', label: 'Nested', hint: 'Tiles inside tiles' },
  ]
</script>

<div class="chart-controls">
  <div class="chart-mode" role="group" aria-label="Chart type">
    {#each families as family (family.value)}
      <button
        type="button"
        class:active={mode === family.value}
        aria-pressed={mode === family.value}
        title={family.hint}
        onclick={() => onModeChange(family.value)}
      >
        <family.icon size={14} aria-hidden="true" />
        <span>{family.label}</span>
      </button>
    {/each}
  </div>
  {#if mode === 'treemap'}
    <div class="chart-mode chart-mode-sub" role="group" aria-label="Treemap density">
      {#each densities as option (option.value)}
        <button
          type="button"
          class:active={density === option.value}
          aria-pressed={density === option.value}
          title={option.hint}
          onclick={() => onDensityChange(option.value)}>{option.label}</button
        >
      {/each}
    </div>
  {/if}
</div>
