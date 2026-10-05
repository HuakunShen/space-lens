<script lang="ts">
  import { Select as SelectPrimitive } from 'bits-ui'
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down'
  import { cn, type WithoutChild } from '../../../lib/utils.js'

  let {
    ref = $bindable(null),
    class: className,
    children,
    size = 'default',
    ...restProps
  }: WithoutChild<SelectPrimitive.TriggerProps> & {
    size?: 'sm' | 'default'
  } = $props()
</script>

<SelectPrimitive.Trigger
  bind:ref
  data-slot="select-trigger"
  data-size={size}
  class={cn(
    "macos:h-7 macos:rounded-[5px] macos:bg-background macos:text-[13px] macos:shadow-inner macos:focus-visible:ring-primary/40 windows:h-8 windows:rounded-[4px] windows:bg-(--control) windows:border-b-foreground/40 windows:focus-visible:border-b-2 windows:focus-visible:border-b-primary windows:focus-visible:ring-0 linux:h-9 linux:rounded-lg linux:bg-(--control) linux:border-transparent linux:shadow-none linux:focus-visible:ring-primary/50 border-input data-placeholder:text-muted-foreground dark:bg-input/30 dark:hover:bg-input/50 focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive dark:aria-invalid:border-destructive/50 gap-1.5 rounded-md border bg-transparent py-2 pr-2 pl-2.5 text-sm shadow-xs transition-[color,box-shadow] focus-visible:ring-3 aria-invalid:ring-3 data-[size=default]:h-9 data-[size=sm]:h-8 *:data-[slot=select-value]:flex *:data-[slot=select-value]:gap-1.5 [&_svg:not([class*='size-'])]:size-4 flex w-fit items-center justify-between whitespace-nowrap outline-none disabled:cursor-not-allowed disabled:opacity-50 *:data-[slot=select-value]:line-clamp-1 *:data-[slot=select-value]:flex *:data-[slot=select-value]:items-center [&_svg]:pointer-events-none [&_svg]:shrink-0",
    'macos:data-[size=sm]:h-6 macos:data-[size=default]:h-7 windows:data-[size=default]:h-8 linux:data-[size=sm]:h-9 linux:data-[size=default]:h-9',
    className,
  )}
  {...restProps}
>
  {@render children?.()}
  <ChevronDownIcon class="text-muted-foreground size-4 pointer-events-none" />
</SelectPrimitive.Trigger>
