<script lang="ts">
  import { Minus, Trash2 } from "@lucide/svelte";
  import type { CollectorEntry } from "../../types";
  import { useLensI18n } from "../../lib/i18n/context.svelte";
  import { Button } from "../ui/button/index";
  import { ScrollArea } from "../ui/scroll-area/index";
  import * as Sheet from "../ui/sheet/index";

  interface Props {
    open: boolean;
    entries: CollectorEntry[];
    totalSize: number;
    deleting: boolean;
    onClose: () => void;
    onRemove: (id: string) => void;
    onDelete: () => void;
    actionLabel?: string;
  }

  let {
    open,
    entries,
    totalSize,
    deleting,
    onClose,
    onRemove,
    onDelete,
    actionLabel,
  }: Props = $props();
  const i18n = useLensI18n();
</script>

<Sheet.Root {open} onOpenChange={(value) => (!value ? onClose() : undefined)}>
  <Sheet.Content class="flex w-full flex-col gap-0 sm:max-w-xl">
    <Sheet.Header class="border-b pb-5">
      <Sheet.Title>{i18n.t('lens.collector.title')}</Sheet.Title>
      <Sheet.Description>
        {i18n.t('lens.collector.summary', { count: i18n.count(entries.length), size: i18n.bytes(totalSize) })}
      </Sheet.Description>
    </Sheet.Header>

    <ScrollArea class="min-h-0 flex-1 py-4">
      {#if entries.length === 0}
        <p
          class="rounded-lg border border-dashed p-6 text-sm text-muted-foreground"
        >
          {i18n.t('lens.collector.empty')}
        </p>
      {:else}
        <div class="grid gap-2 pr-3">
          {#each entries as entry (entry.id)}
            <div
              class="grid grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-3 rounded-lg border bg-card p-3"
            >
              <div class="min-w-0">
                <strong class="block truncate text-sm">{entry.name}</strong>
                <span class="block truncate text-xs text-muted-foreground"
                  >{entry.path}</span
                >
              </div>
              <span class="text-sm font-medium tabular-nums"
                >{i18n.bytes(entry.size)}</span
              >
              <Button
                variant="ghost"
                size="icon-sm"
                type="button"
                onclick={() => onRemove(entry.id)}
                aria-label={i18n.t('lens.collector.remove', { name: entry.name })}
              >
                <Minus class="size-4" />
              </Button>
            </div>
          {/each}
        </div>
      {/if}
    </ScrollArea>

    <Sheet.Footer class="border-t pt-4">
      <Button variant="outline" type="button" onclick={onClose}
        >{i18n.t('lens.collector.later')}</Button
      >
      <Button
        variant="destructive"
        type="button"
        onclick={onDelete}
        disabled={entries.length === 0 || deleting}
      >
        <Trash2 class="size-4" />
        {actionLabel ?? i18n.t(deleting ? 'lens.collector.deleting' : 'lens.collector.delete')}
      </Button>
    </Sheet.Footer>
  </Sheet.Content>
</Sheet.Root>
