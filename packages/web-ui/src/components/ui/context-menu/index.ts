import { ContextMenu as ContextMenuPrimitive } from 'bits-ui'
import Content from './context-menu-content.svelte'
import Item from './context-menu-item.svelte'
const Root = ContextMenuPrimitive.Root
const Trigger = ContextMenuPrimitive.Trigger
const Group = ContextMenuPrimitive.Group
const Portal = ContextMenuPrimitive.Portal

export {
  Root,
  Trigger,
  Group,
  Portal,
  Content,
  Item,
  //
  Root as ContextMenuRoot,
  Trigger as ContextMenuTrigger,
  Group as ContextMenuGroup,
  Portal as ContextMenuPortal,
  Content as ContextMenuContent,
  Item as ContextMenuItem,
}
