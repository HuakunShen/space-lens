<script lang="ts">
  import { ChevronLeft, ChevronRight, Home } from "@lucide/svelte";
  import type { TreeNodeSummary } from "../../types";
  import { Button } from "../ui/button/index";
  import { useLensI18n } from "../../lib/i18n/context.svelte";

  interface Props {
    items: TreeNodeSummary[];
    onSelect: (node: TreeNodeSummary) => void;
    onBack?: () => void;
    canGoBack?: boolean;
  }

  let { items, onSelect, onBack, canGoBack = false }: Props = $props();
  const i18n = useLensI18n();
</script>

<nav class="flex min-w-0 items-center gap-1" aria-label={i18n.t('lens.breadcrumb.label')}>
  <Button
    variant="ghost"
    size="icon-xs"
    type="button"
    onclick={onBack}
    disabled={!canGoBack}
    aria-label={i18n.t('lens.breadcrumb.back')}
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
