<script lang="ts">
  import { ChevronRight, Plus } from "@lucide/svelte";
  import type { TreeNodeSummary } from "$lib/api/types";
  import { nodeColor } from "$lib/chart/colors";
  import { formatBytes } from "$lib/format";
  import { Button } from "$lib/components/ui/button/index.js";
  import { ScrollArea } from "$lib/components/ui/scroll-area/index.js";

  interface Props {
    items: TreeNodeSummary[];
    hoveredId: string | null;
    collectedIds: Set<string>;
    onHover: (id: string | null) => void;
    onOpen: (node: TreeNodeSummary) => void;
    onCollect: (node: TreeNodeSummary) => void;
    onContext: (node: TreeNodeSummary, x: number, y: number) => void;
  }

  let {
    items,
    hoveredId,
    collectedIds,
    onHover,
    onOpen,
    onCollect,
    onContext,
  }: Props = $props();
</script>

<ScrollArea class="min-h-0 flex-1 pr-2" aria-label="Directory children">
  <div class="grid gap-2" role="list">
    {#each items as item, index (item.id)}
      <div
        class={[
          "group grid min-h-14 grid-cols-[auto_minmax(0,1fr)_auto_auto] items-center gap-3 rounded-lg border bg-card/45 px-3 py-2 text-left transition-colors hover:border-ring hover:bg-card/70",
          hoveredId === item.id
            ? "border-primary/30 bg-primary/5"
            : "border-border/70",
          collectedIds.has(item.id)
            ? "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300"
            : "",
        ]}
        role="listitem"
        data-child-row={item.id}
        onmouseenter={() => onHover(item.id)}
        onmouseleave={() => onHover(null)}
        oncontextmenu={(event) => {
          event.preventDefault();
          onContext(item, event.clientX, event.clientY);
        }}
        title={item.path}
      >
        <span
          class="size-2.5 rounded-full shadow-[0_0_14px_var(--node)]"
          style={`--node: ${nodeColor(item.id, item.depth, index)}; background: var(--node)`}
        ></span>
        <button
          class="min-w-0 text-left"
          type="button"
          onclick={() => onOpen(item)}
        >
          <span class="block truncate text-sm font-medium">{item.name}</span>
          <span class="block truncate text-xs text-muted-foreground"
            >{item.childCount} items</span
          >
        </button>
        <span class="text-xs font-medium tabular-nums text-muted-foreground"
          >{formatBytes(item.size)}</span
        >
        <div class="flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon-sm"
            type="button"
            onclick={() => onCollect(item)}
            aria-label={`Move ${item.name} to Collector`}
          >
            <Plus class="size-4" />
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
    {/each}
  </div>
</ScrollArea>
