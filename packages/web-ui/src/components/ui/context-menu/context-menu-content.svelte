<script lang="ts">
  import { ContextMenu as ContextMenuPrimitive } from "bits-ui";
  import type { Snippet } from "svelte";
  import { cn, type WithoutChildrenOrChild } from "../../../lib/utils";

  let {
    ref = $bindable(null),
    class: className,
    children,
    ...restProps
  }: WithoutChildrenOrChild<ContextMenuPrimitive.ContentProps> & {
    children: Snippet;
  } = $props();
</script>

<ContextMenuPrimitive.Portal>
  <ContextMenuPrimitive.Content
    bind:ref
    data-slot="context-menu-content"
    class={cn(
      "bg-popover text-popover-foreground data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 z-50 min-w-[8rem] overflow-hidden rounded-md border p-1 text-sm shadow-md",
      className,
    )}
    {...restProps}
  >
    {@render children?.()}
  </ContextMenuPrimitive.Content>
</ContextMenuPrimitive.Portal>
