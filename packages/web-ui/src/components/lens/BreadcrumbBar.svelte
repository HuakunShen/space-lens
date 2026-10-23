<script lang="ts">
  import { ChevronLeft, ChevronRight, Home, ExternalLink } from "@lucide/svelte";
  import type { TreeNodeSummary } from "../../types";
  import { Button } from "../ui/button/index";

  interface Props {
    items: TreeNodeSummary[];
    onSelect: (node: TreeNodeSummary) => void;
    onBack?: () => void;
    canGoBack?: boolean;
    /**
     * Shows the current folder in the host's own file manager (Finder /
     * Explorer / the Linux file manager). Absent when the host cannot do it.
     */
    onReveal?: (node: TreeNodeSummary, action: "reveal" | "open") => void;
    /** The label for that action, named after the host's platform. */
    revealLabel?: string;
  }

  let {
    items,
    onSelect,
    onBack,
    canGoBack = false,
    onReveal,
    revealLabel = "Reveal in file manager",
  }: Props = $props();

  const current = $derived(items.at(-1));
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
  {#if onReveal && current}
    {@const target = current}
    <Button
      variant="ghost"
      size="icon-xs"
      type="button"
      onclick={() => onReveal(target, "reveal")}
      aria-label={`Reveal ${target.name} in the file manager`}
      title={revealLabel}
    >
      <ExternalLink class="size-3.5" />
    </Button>
  {/if}
</nav>
