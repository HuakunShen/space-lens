<script lang="ts">
  import { ChevronLeft, ChevronRight, Home } from "@lucide/svelte";
  import type { TreeNodeSummary } from "$lib/api/types";
  import { Button } from "$lib/components/ui/button/index.js";

  interface Props {
    items: TreeNodeSummary[];
    onSelect: (node: TreeNodeSummary) => void;
    onBack?: () => void;
    canGoBack?: boolean;
  }

  let { items, onSelect, onBack, canGoBack = false }: Props = $props();
</script>

<nav class="flex min-w-0 items-center gap-1" aria-label="Current path">
  <Button
    variant="ghost"
    size="icon-xs"
    type="button"
    onclick={onBack}
    disabled={!canGoBack}
    aria-label="Back"
  >
    <ChevronLeft class="size-3.5" />
  </Button>
  <div class="flex min-w-0 items-center gap-1 overflow-hidden">
    {#each items as item, index (item.id)}
      {#if index > 0}
        <ChevronRight class="size-3 shrink-0 text-muted-foreground" />
      {/if}
      <Button
        variant={index === items.length - 1 ? "secondary" : "ghost"}
        size="xs"
        type="button"
        class="max-w-36 shrink truncate px-2"
        onclick={() => onSelect(item)}
        title={item.path}
      >
        {#if index === 0}
          <Home class="size-3.5" />
        {/if}
        {item.name}
      </Button>
    {/each}
  </div>
</nav>
