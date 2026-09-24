<script lang="ts">
  import { ContextMenu as ContextMenuPrimitive } from "bits-ui";
  import { cn, type WithoutChild } from "../../../lib/utils";

  let {
    ref = $bindable(null),
    class: className,
    inset,
    ...restProps
  }: WithoutChild<ContextMenuPrimitive.ItemProps> & {
    inset?: boolean;
  } = $props();
</script>

<ContextMenuPrimitive.Item
  bind:ref
  data-slot="context-menu-item"
  data-inset={inset}
  class={cn(
    "focus:bg-accent focus:text-accent-foreground relative flex cursor-default items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-hidden select-none data-disabled:pointer-events-none data-disabled:opacity-50 data-inset:pl-8 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
    className,
  )}
  {...restProps}
/>
