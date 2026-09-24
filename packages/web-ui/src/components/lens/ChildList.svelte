<script lang="ts">
  import { ChevronRight, FolderOpen, Minus, Plus } from '@lucide/svelte'
  import type { TreeNodeSummary } from '../../types'
  import { nodeColor, nodeMutedColor } from '../../lib/colors'
  import { formatBytes } from '../../lib/format'
  import * as ContextMenu from '../ui/context-menu/index'
  import { Button } from '../ui/button/index'
  import { ScrollArea } from '../ui/scroll-area/index'

  interface Props {
    items: TreeNodeSummary[]
    totalSize?: number
    hoveredId: string | null
    collectedIds: Set<string>
    onHover: (id: string | null) => void
    onOpen: (node: TreeNodeSummary) => void
    onCollect: (node: TreeNodeSummary) => void
    onRemove: (node: TreeNodeSummary) => void
  }

  let { items, totalSize = 0, hoveredId, collectedIds, onHover, onOpen, onCollect, onRemove }: Props = $props()

  // One collector verb per row state: a collected row offers removal, the rest
  // offer collection — the button, the menu item and their labels stay in step.
  function isCollected(item: TreeNodeSummary): boolean {
    return collectedIds.has(item.id)
  }
  function toggle(item: TreeNodeSummary): void {
    if (isCollected(item)) onRemove(item)
    else onCollect(item)
  }
  function collectLabel(item: TreeNodeSummary): string {
    return isCollected(item) ? `Remove ${item.name} from Collector` : `Move ${item.name} to Collector`
  }
</script>

<ScrollArea class="min-h-0 flex-1 pr-2" aria-label="Directory children">
  <div class="child-list" role="list">
    {#each items as item (item.id)}
      <ContextMenu.Root>
        <ContextMenu.Trigger>
          {#snippet child({ props })}
            <div
              {...props}
              class={[
                'child-row group grid min-h-14 cursor-default grid-cols-[auto_minmax(0,1fr)_auto_auto] items-center gap-3 rounded-lg border px-3 py-2 text-left',
                hoveredId === item.id ? 'border-primary/30 bg-primary/5' : 'border-border/70',
                isCollected(item)
                  ? 'border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300'
                  : '',
              ]}
              role="listitem"
              data-child-row={item.id}
              onmouseenter={() => onHover(item.id)}
              onmouseleave={() => onHover(null)}
              onfocusin={() => onHover(item.id)}
              onfocusout={(event) => {
                if (
                  !(event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget))
                )
                  onHover(null)
              }}
              title={item.path}
            >
              <span
                class="size-2.5 rounded-full"
                style={`--node: ${item.ignored ? nodeMutedColor(item.depth) : nodeColor(item.id, item.depth)}; background: var(--node)`}
              ></span>
              <button class="min-w-0 text-left" type="button" onclick={() => onOpen(item)}>
                <span class="block truncate text-sm font-medium">{item.name}</span>
                <span class="block truncate text-xs text-muted-foreground"
                  >{item.hasChildren
                    ? `${item.childCount.toLocaleString()} ${item.childCount === 1 ? 'item' : 'items'}`
                    : 'File'}</span
                >
              </button>
              <div class="child-size">
                <span>{formatBytes(item.size)}</span>
                {#if totalSize > 0}
                  <div class="child-meter" aria-hidden="true">
                    <span
                      style={`width: ${totalSize > 0 ? Math.min(100, (item.size / totalSize) * 100) : 0}%; background: ${item.ignored ? nodeMutedColor(item.depth) : nodeColor(item.id, item.depth)}`}
                    ></span>
                  </div>
                {/if}
              </div>
              <div class="flex items-center gap-1">
                <Button
                  variant="ghost"
                  size="icon-sm"
                  type="button"
                  onclick={() => toggle(item)}
                  aria-label={collectLabel(item)}
                >
                  {#if isCollected(item)}<Minus class="size-4" />{:else}<Plus class="size-4" />{/if}
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  type="button"
                  onclick={() => onOpen(item)}
                  aria-label={`Open ${item.name}`}
                >
                  <ChevronRight class="size-4" />
                </Button>
              </div>
            </div>
          {/snippet}
        </ContextMenu.Trigger>
        <ContextMenu.Content>
          <ContextMenu.Item onclick={() => toggle(item)}>
            {#if isCollected(item)}<Minus />{:else}<Plus />{/if}
            {collectLabel(item)}
          </ContextMenu.Item>
          <ContextMenu.Item onclick={() => onOpen(item)}>
            <FolderOpen />
            Open {item.name}
          </ContextMenu.Item>
        </ContextMenu.Content>
      </ContextMenu.Root>
    {:else}
      <p class="px-4 py-8 text-center text-sm text-muted-foreground">No child items in this view.</p>
    {/each}
  </div>
</ScrollArea>
