<script lang="ts">
  import {
    ArrowRight,
    FolderOpen,
    HardDrive,
    Plus,
    Search,
  } from "@lucide/svelte";
  import type { ScanStatus, ScanTarget } from "$lib/api/types";
  import { formatBytes } from "$lib/format";
  import { Badge } from "$lib/components/ui/badge/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import * as Card from "$lib/components/ui/card/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { Progress } from "$lib/components/ui/progress/index.js";

  interface Props {
    targets: ScanTarget[];
    mode: string;
    busy: boolean;
    error: string | null;
    status: ScanStatus | null;
    onScan: (paths: string[]) => void;
    onCancel: () => void;
  }

  let { targets, mode, busy, error, status, onScan, onCancel }: Props =
    $props();
  let selectedId = $state("");
  let customPath = $state("");
  let secondPath = $state("");
  let selectedTarget = $derived(
    targets.find((target) => target.id === selectedId),
  );
  let selectedPaths = $derived(
    selectedTarget
      ? [selectedTarget.path]
      : [customPath, secondPath].map((path) => path.trim()).filter(Boolean),
  );
  let canScan = $derived(selectedPaths.length > 0 && !busy);
  let isKunkunMode = $derived(mode === "kunkun");

  $effect(() => {
    if (selectedId === "custom") return;
    if (targets.some((target) => target.id === selectedId)) return;
    selectedId = targets[0]?.id ?? "";
  });

  function scanSelected() {
    if (!canScan) return;
    onScan(selectedPaths);
  }
</script>

<main
  class="grid h-dvh min-h-0 bg-background/70 text-foreground backdrop-blur-2xl"
>
  <section class="flex h-full min-h-0 w-full flex-col">
    <header
      class={[
        "flex min-h-12 items-center justify-between gap-4 border-b px-4 py-2 [-webkit-app-region:drag]",
        isKunkunMode ? "pl-24" : "",
      ]}
    >
      <div class="flex min-w-0 items-center gap-2.5">
        <div
          class="grid size-8 shrink-0 place-items-center rounded-lg bg-primary text-primary-foreground"
        >
          <Search size={15} />
        </div>
        <div class="min-w-0">
          <h1 class="truncate text-base font-semibold leading-tight">
            Space Lens
          </h1>
        </div>
      </div>
      <Badge variant="outline" class="[-webkit-app-region:no-drag]">
        {mode}
      </Badge>
    </header>

    <div class="grid min-h-0 flex-1 grid-cols-[280px_minmax(0,1fr)]">
      <aside class="flex min-h-0 flex-col gap-3 border-r bg-sidebar/55 p-3">
        <div class="grid gap-1.5">
          <span
            class="px-2 text-xs font-semibold uppercase text-muted-foreground"
          >
            Disks and Folders
          </span>
          {#each targets as target (target.id)}
            <button
              class={[
                "flex min-h-12 items-center gap-2 rounded-md border border-transparent px-2.5 text-left text-sm text-muted-foreground transition-colors hover:border-border hover:bg-accent hover:text-accent-foreground",
                selectedId === target.id
                  ? "border-border bg-accent text-accent-foreground"
                  : "",
              ]}
              onclick={() => (selectedId = target.id)}
              type="button"
            >
              {#if target.kind === "volume"}
                <HardDrive size={15} />
              {:else}
                <FolderOpen size={15} />
              {/if}
              <span class="min-w-0 flex-1">
                <span class="block truncate font-medium">{target.label}</span>
                <span class="block truncate text-xs text-muted-foreground">
                  {target.description}
                </span>
              </span>
            </button>
          {/each}
          <button
            class={[
              "flex min-h-12 items-center gap-2 rounded-md border border-dashed px-2.5 text-left text-sm text-muted-foreground transition-colors hover:border-border hover:bg-accent hover:text-accent-foreground",
              selectedId === "custom"
                ? "border-border bg-accent text-accent-foreground"
                : "",
            ]}
            onclick={() => (selectedId = "custom")}
            type="button"
          >
            <Plus size={15} />
            <span class="min-w-0 flex-1">
              <span class="block truncate font-medium">Choose Folder</span>
              <span class="block truncate text-xs text-muted-foreground">
                Enter a local path
              </span>
            </span>
          </button>
        </div>
      </aside>

      <section class="flex min-h-0 flex-col bg-background/45 p-4">
        <div class="grid min-h-0 flex-1 place-items-center">
          <Card.Root class="w-full max-w-2xl border bg-card/50 shadow-none">
            <Card.Header>
              <div class="flex items-start justify-between gap-3">
                <div class="min-w-0">
                  <Card.Title class="truncate text-xl">
                    {selectedTarget?.label ?? "Choose Folder"}
                  </Card.Title>
                  <Card.Description class="truncate">
                    {selectedTarget?.path ??
                      "Scan one folder or group multiple folders under one root."}
                  </Card.Description>
                </div>
                <Badge variant="outline" class="capitalize">
                  {selectedTarget?.kind.replace("-", " ") ?? "folder"}
                </Badge>
              </div>
            </Card.Header>

            {#if !selectedTarget}
              <Card.Content class="grid gap-3">
                <label class="grid gap-1.5">
                  <span
                    class="text-xs font-semibold uppercase text-muted-foreground"
                  >
                    Folder path
                  </span>
                  <Input bind:value={customPath} placeholder="/path/to/folder" />
                </label>
                <label class="grid gap-1.5">
                  <span
                    class="text-xs font-semibold uppercase text-muted-foreground"
                  >
                    Optional second folder
                  </span>
                  <Input
                    bind:value={secondPath}
                    placeholder="/path/to/another-folder"
                  />
                </label>
              </Card.Content>
            {:else}
              <Card.Content>
                <div class="rounded-md border bg-background/60 p-3">
                  <span
                    class="text-xs font-semibold uppercase text-muted-foreground"
                  >
                    Scan root
                  </span>
                  <p class="mt-1 break-all font-mono text-sm">
                    {selectedTarget.path}
                  </p>
                </div>
              </Card.Content>
            {/if}

            <Card.Footer class="justify-between gap-3 border-t bg-muted/20">
              <p class="text-sm text-muted-foreground">
                The browser loads only visible tree slices.
              </p>
              <Button type="button" onclick={scanSelected} disabled={!canScan}>
                {busy ? "Scanning" : "Scan"}
                <ArrowRight size={15} />
              </Button>
            </Card.Footer>
          </Card.Root>

          {#if status?.state === "scanning"}
            <Card.Root
              class="mt-4 w-full max-w-2xl border bg-card/50 shadow-none"
            >
              <Card.Header class="space-y-2">
                <div class="flex items-center justify-between gap-3">
                  <div class="min-w-0">
                    <Card.Title class="truncate text-base">
                      Building storage map
                    </Card.Title>
                    <Card.Description class="truncate">
                      {formatBytes(status.bytesScanned)} scanned across {status.entriesScanned}
                      entries
                    </Card.Description>
                  </div>
                  <Button
                    type="button"
                    size="sm"
                    variant="secondary"
                    onclick={onCancel}
                  >
                    Stop
                  </Button>
                </div>
                <Progress
                  value={status.progress === null
                    ? 35
                    : Math.round(status.progress * 100)}
                  class="[&_[data-slot=progress-indicator]]:animate-pulse"
                />
              </Card.Header>
              <Card.Content>
                <p class="truncate font-mono text-xs text-muted-foreground">
                  {status.currentPath ?? "Preparing scanner..."}
                </p>
              </Card.Content>
            </Card.Root>
          {/if}
        </div>

        {#if error}
          <div
            class="mt-3 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
          >
            {error}
          </div>
        {/if}
      </section>
    </div>
  </section>
</main>
