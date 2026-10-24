<script lang="ts">
  import { LinkPreview as HoverCardPrimitive } from 'bits-ui'
  import type { ComponentProps } from 'svelte'
  import { cn, type WithoutChildrenOrChild } from '../../../lib/utils'
  import HoverCardPortal from './hover-card-portal.svelte'

  let {
    ref = $bindable(null),
    class: className,
    align = 'center',
    sideOffset = 4,
    portalProps,
    ...restProps
  }: HoverCardPrimitive.ContentProps & {
    portalProps?: WithoutChildrenOrChild<ComponentProps<typeof HoverCardPortal>>
  } = $props()
</script>

<HoverCardPortal {...portalProps}>
  <HoverCardPrimitive.Content
    bind:ref
    data-slot="hover-card-content"
    {align}
    {sideOffset}
    class={cn(
      'macos:rounded-lg macos:bg-popover/95 macos:backdrop-blur-xl macos:shadow-xl bg-popover text-popover-foreground data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 z-50 w-64 rounded-md border p-3 shadow-md outline-hidden',
      className,
    )}
    {...restProps}
  />
</HoverCardPortal>
