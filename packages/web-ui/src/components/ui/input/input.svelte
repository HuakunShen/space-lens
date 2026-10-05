<script lang="ts">
  import type { HTMLInputAttributes, HTMLInputTypeAttribute } from 'svelte/elements'
  import { cn, type WithElementRef } from '../../../lib/utils'

  type InputType = Exclude<HTMLInputTypeAttribute, 'file'>

  type Props = WithElementRef<
    Omit<HTMLInputAttributes, 'type'> & ({ type: 'file'; files?: FileList } | { type?: InputType; files?: undefined })
  >

  let {
    ref = $bindable(null),
    value = $bindable(),
    type,
    files = $bindable(),
    class: className,
    'data-slot': dataSlot = 'input',
    ...restProps
  }: Props = $props()
</script>

{#if type === 'file'}
  <input
    bind:this={ref}
    data-slot={dataSlot}
    class={cn(
      'macos:h-7 macos:rounded-[5px] macos:bg-background macos:text-[13px] macos:shadow-inner macos:focus-visible:ring-primary/40 windows:h-8 windows:rounded-[4px] windows:bg-(--control) windows:border-b-foreground/40 windows:focus-visible:border-b-2 windows:focus-visible:border-b-primary windows:focus-visible:ring-0 linux:h-9 linux:rounded-lg linux:bg-(--control) linux:border-transparent linux:shadow-none linux:focus-visible:ring-primary/50 dark:bg-input/30 border-input focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive dark:aria-invalid:border-destructive/50 h-9 rounded-md border bg-transparent px-2.5 py-1 text-base shadow-xs transition-[color,box-shadow] file:h-7 file:text-sm file:font-medium focus-visible:ring-3 aria-invalid:ring-3 md:text-sm file:text-foreground placeholder:text-muted-foreground w-full min-w-0 outline-none file:inline-flex file:border-0 file:bg-transparent disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50',
      className,
    )}
    type="file"
    bind:files
    bind:value
    {...restProps}
  />
{:else}
  <input
    bind:this={ref}
    data-slot={dataSlot}
    class={cn(
      'macos:h-7 macos:rounded-[5px] macos:bg-background macos:text-[13px] macos:shadow-inner macos:focus-visible:ring-primary/40 windows:h-8 windows:rounded-[4px] windows:bg-(--control) windows:border-b-foreground/40 windows:focus-visible:border-b-2 windows:focus-visible:border-b-primary windows:focus-visible:ring-0 linux:h-9 linux:rounded-lg linux:bg-(--control) linux:border-transparent linux:shadow-none linux:focus-visible:ring-primary/50 dark:bg-input/30 border-input focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive dark:aria-invalid:border-destructive/50 h-9 rounded-md border bg-transparent px-2.5 py-1 text-base shadow-xs transition-[color,box-shadow] file:h-7 file:text-sm file:font-medium focus-visible:ring-3 aria-invalid:ring-3 md:text-sm file:text-foreground placeholder:text-muted-foreground w-full min-w-0 outline-none file:inline-flex file:border-0 file:bg-transparent disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50',
      className,
    )}
    {type}
    bind:value
    {...restProps}
  />
{/if}
