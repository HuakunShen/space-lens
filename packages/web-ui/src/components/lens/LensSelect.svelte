<script lang="ts">
  import * as Select from '../ui/select/index.js'
  import { cn } from '../../lib/utils.js'

  interface Props {
    value: string
    options: { value: string; label: string }[]
    label: string
    class?: string
    disabled?: boolean
    onChange: (value: string) => void
  }
  let { value, options, label, class: className, disabled = false, onChange }: Props = $props()
</script>

<Select.Root type="single" {value} items={options} {disabled} allowDeselect={false} onValueChange={onChange}>
  <Select.Trigger size="sm" aria-label={label} class={cn('gap-3 text-xs', className)}>
    <Select.Value />
  </Select.Trigger>
  <Select.Content>
    <Select.Group>
      {#each options as option (option.value)}
        <Select.Item value={option.value} label={option.label} />
      {/each}
    </Select.Group>
  </Select.Content>
</Select.Root>
