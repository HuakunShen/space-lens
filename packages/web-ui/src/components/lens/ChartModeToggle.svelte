<script lang="ts">
  import type { ChartMode } from '../../lib/treemap'

  interface Props {
    mode: ChartMode
    onModeChange: (mode: ChartMode) => void
  }
  let { mode, onModeChange }: Props = $props()

  const options: Array<{ value: ChartMode; label: string; hint: string }> = [
    { value: 'sunburst', label: 'Sunburst', hint: 'Ring chart' },
    { value: 'flat', label: 'Flat', hint: 'One layer of tiles' },
    { value: 'nested', label: 'Nested', hint: 'Tiles inside tiles' },
  ]
</script>

<div class="chart-mode" role="group" aria-label="Chart mode">
  {#each options as option (option.value)}
    <button
      type="button"
      class:active={mode === option.value}
      aria-pressed={mode === option.value}
      title={option.hint}
      onclick={() => onModeChange(option.value)}>{option.label}</button
    >
  {/each}
</div>
