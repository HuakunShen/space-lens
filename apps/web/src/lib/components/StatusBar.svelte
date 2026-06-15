<script lang="ts">
  import { LoaderCircle, Octagon, PackageOpen } from "@lucide/svelte";
  import type { ScanStatus } from "$lib/api/types";
  import { formatBytes, formatPercent } from "$lib/format";
  import { Badge } from "$lib/components/ui/badge/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Progress } from "$lib/components/ui/progress/index.js";

  interface Props {
    status: ScanStatus | null;
    collectorTotal: number;
    collectorCount: number;
    onOpenCollector: () => void;
    onCancel: () => void;
  }

  let {
    status,
    collectorTotal,
    collectorCount,
    onOpenCollector,
    onCancel,
  }: Props = $props();
</script>

<footer class="border-t bg-background/55 px-4 py-2 backdrop-blur-xl">
  <div class="grid gap-3 md:grid-cols-[1fr_auto_auto] md:items-center">
    <div class="min-w-0 space-y-2">
      <div class="flex items-center gap-2 text-sm">
        {#if status?.state === "scanning"}
          <LoaderCircle class="size-4 animate-spin text-primary" />
        {/if}
        <span class="truncate text-muted-foreground"
          >{status?.message ?? "Ready"}</span
        >
        {#if status?.state === "scanning"}
          <span class="shrink-0 text-muted-foreground">
            {formatBytes(status.bytesScanned)}
          </span>
        {/if}
        {#if status?.progress !== null && status?.progress !== undefined}
          <Badge variant="secondary">{formatPercent(status.progress)}</Badge>
        {/if}
      </div>
      {#if status?.progress !== null && status?.progress !== undefined}
        <Progress value={Math.round(status.progress * 100)} class="max-w-md" />
      {:else if status?.state === "scanning"}
        <Progress
          value={35}
          class="max-w-md [&_[data-slot=progress-indicator]]:animate-pulse"
        />
        <p class="truncate font-mono text-xs text-muted-foreground">
          {status.currentPath ?? "Preparing scanner..."}
        </p>
      {/if}
    </div>
    <Button
      variant="secondary"
      size="sm"
      type="button"
      onclick={onOpenCollector}
      class="justify-start"
    >
      <PackageOpen class="size-4" />
      Collector: {collectorCount} / {formatBytes(collectorTotal)}
    </Button>
    <Button
      variant="ghost"
      size="sm"
      type="button"
      onclick={onCancel}
      disabled={status?.state !== "scanning"}
    >
      <Octagon class="size-4" />
      Stop
    </Button>
  </div>
</footer>
