<script lang="ts">
  import { Progress as ProgressPrimitive } from "bits-ui";
  import { cn, type WithoutChildrenOrChild } from "$lib/utils.js";

  type Props = WithoutChildrenOrChild<ProgressPrimitive.RootProps> & {
    indeterminate?: boolean;
  };

  let {
    ref = $bindable(null),
    class: className,
    max = 100,
    value,
    indeterminate = false,
    ...restProps
  }: Props = $props();

  const effectiveValue = $derived(indeterminate ? undefined : value);
  const indicatorStyle = $derived(
    indeterminate
      ? undefined
      : `transform: translateX(-${100 - (100 * (value ?? 0)) / (max ?? 1)}%)`,
  );
</script>

<ProgressPrimitive.Root
  bind:ref
  data-slot="progress"
  data-indeterminate={indeterminate ? "" : undefined}
  class={cn(
    "bg-muted h-1.5 rounded-full relative flex w-full items-center overflow-x-hidden",
    className,
  )}
  value={effectiveValue}
  {max}
  {...restProps}
>
  <div
    data-slot="progress-indicator"
    class={cn(
      "bg-primary size-full flex-1 transition-all",
      indeterminate && "space-lens-progress-indeterminate",
    )}
    style={indicatorStyle}
  ></div>
</ProgressPrimitive.Root>

<style>
  .space-lens-progress-indeterminate {
    width: 38%;
    flex: none;
    animation: space-lens-progress-indeterminate 1.25s ease-in-out infinite;
  }

  @keyframes space-lens-progress-indeterminate {
    0% {
      transform: translateX(-120%);
    }
    100% {
      transform: translateX(280%);
    }
  }
</style>
