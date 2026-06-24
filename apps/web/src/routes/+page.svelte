<script lang="ts">
  import { onMount } from "svelte";
  import { FolderSearch, PackageOpen, RotateCcw } from "@lucide/svelte";
  import BreadcrumbBar from "$lib/components/BreadcrumbBar.svelte";
  import ChildList from "$lib/components/ChildList.svelte";
  import CollectorPanel from "$lib/components/CollectorPanel.svelte";
  import ScanPicker from "$lib/components/ScanPicker.svelte";
  import StatusBar from "$lib/components/StatusBar.svelte";
  import SunburstChart from "$lib/components/SunburstChart.svelte";
  import { createSpaceLensClient } from "$lib/api/client";
  import type { SpaceLensClient } from "$lib/api/client";
  import type {
    CollectorEntry,
    ScanSession,
    ScanStatus,
    ScanTarget,
    TreeNodeSummary,
    TreeSlice,
  } from "$lib/api/types";
  import { formatBytes } from "$lib/format";
  import { Badge } from "$lib/components/ui/badge/index.js";
  import { Button } from "$lib/components/ui/button/index.js";

  const visibleDepth = 3;
  const maxChildrenPerNode = 28;
  const rightPanelPageSize = 100;

  let client = $state<SpaceLensClient | null>(null);
  let initializing = $state(true);
  let session = $state<ScanSession | null>(null);
  let slice = $state<TreeSlice | null>(null);
  let status = $state<ScanStatus | null>(null);
  let busy = $state(false);
  let childLoading = $state(false);
  let childItems = $state<TreeNodeSummary[]>([]);
  let childPageTotal = $state(0);
  let childPageOffset = $state(0);
  let deleting = $state(false);
  let error = $state<string | null>(null);
  let hoveredId = $state<string | null>(null);
  let collectorOpen = $state(false);
  let collectorEntries = $state<CollectorEntry[]>([]);
  let scanTargets = $state<ScanTarget[]>([]);
  let contextMenu = $state<{
    node: TreeNodeSummary;
    x: number;
    y: number;
  } | null>(null);

  let collectorTotal = $derived(
    collectorEntries.reduce((total, entry) => total + entry.size, 0),
  );
  let collectedIds = $derived(
    new Set(collectorEntries.map((entry) => entry.nodeId)),
  );
  let breadcrumbs = $derived(slice?.ancestors ?? []);
  let currentChildren = $derived(childItems);
  let canLoadMoreChildren = $derived(childPageOffset < childPageTotal);
  let isKunkunMode = $derived(client?.mode === "kunkun");
  let hoverInfo = $derived(
    hoveredId && slice
      ? (findTreeNode(slice.tree, hoveredId) ??
          currentChildren.find((item) => item.id === hoveredId) ??
          null)
      : null,
  );
  let chartInfo = $derived(hoverInfo ?? slice?.focusNode ?? null);

  onMount(() => {
    void initializeClient();
  });

  async function initializeClient() {
    initializing = true;
    error = null;
    try {
      client = await createSpaceLensClient();
      await loadScanTargets();
    } catch (cause) {
      error =
        cause instanceof Error ? cause.message : "Unable to start Space Lens";
    } finally {
      initializing = false;
    }
  }

  function requireClient(): SpaceLensClient {
    if (!client) {
      throw new Error("Space Lens is still starting");
    }
    return client;
  }

  async function loadScanTargets() {
    try {
      const activeClient = requireClient();
      scanTargets = await activeClient.api.getScanTargets();
    } catch (cause) {
      error =
        cause instanceof Error ? cause.message : "Unable to load scan targets";
    }
  }

  async function forgetScanTarget(path: string) {
    const activeClient = requireClient();
    if (!activeClient.api.forgetScanTarget) return;
    error = null;
    try {
      await activeClient.api.forgetScanTarget(path);
      await loadScanTargets();
    } catch (cause) {
      error =
        cause instanceof Error
          ? cause.message
          : "Unable to remove recent scan path";
    }
  }

  async function returnToScanPicker() {
    session = null;
    slice = null;
    status = null;
    collectorEntries = [];
    contextMenu = null;
    await loadScanTargets();
  }

  async function startScan(paths: string[]) {
    const activeClient = requireClient();
    busy = true;
    error = null;
    session = null;
    slice = null;
    status = null;
    contextMenu = null;
    collectorEntries = [];
    childItems = [];
    childPageTotal = 0;
    childPageOffset = 0;
    try {
      const nextSession = await activeClient.api.startScan({
        paths,
        ignoreHidden: false,
        respectGitignore: true,
        ignoredMode: "summarize",
        initialDepth: visibleDepth,
        maxChildrenPerNode,
      });
      session = nextSession;
      status = await activeClient.api.getScanStatus(nextSession.scanId);
      const readyStatus = await waitForScanReady(nextSession.scanId);
      const readySession = {
        ...nextSession,
        rootIds: readyStatus.rootIds,
        label: readyStatus.label ?? nextSession.label,
      };
      session = readySession;
      await openNode(readySession.rootIds[0] ?? "root");
    } catch (cause) {
      error = cause instanceof Error ? cause.message : "Scan failed";
    } finally {
      busy = false;
    }
  }

  async function waitForScanReady(scanId: string): Promise<ScanStatus> {
    const activeClient = requireClient();
    while (true) {
      const nextStatus = await activeClient.api.getScanStatus(scanId);
      status = nextStatus;
      if (nextStatus.state === "ready") return nextStatus;
      if (nextStatus.state === "failed") {
        throw new Error(nextStatus.message || "Scan failed");
      }
      if (nextStatus.state === "cancelled") {
        throw new Error("Scan cancelled");
      }
      await sleep(200);
    }
  }

  async function openNode(node: TreeNodeSummary | string) {
    if (!session) return;
    const activeClient = requireClient();
    const nodeId = typeof node === "string" ? node : node.id;
    busy = true;
    error = null;
    contextMenu = null;
    try {
      const [nextSlice, nextStatus, firstChildren] = await Promise.all([
        activeClient.api.getNode({
          scanId: session.scanId,
          nodeId,
          depth: visibleDepth,
          maxChildrenPerNode,
        }),
        activeClient.api.getScanStatus(session.scanId),
        activeClient.api.getChildren({
          scanId: session.scanId,
          nodeId,
          offset: 0,
          limit: rightPanelPageSize,
          sort: "size",
        }),
      ]);
      slice = nextSlice;
      status = nextStatus;
      childItems = firstChildren.items;
      childPageTotal = firstChildren.total;
      childPageOffset = firstChildren.offset + firstChildren.items.length;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : "Unable to load folder";
    } finally {
      busy = false;
    }
  }

  async function loadMoreChildren() {
    if (!session || !slice || childLoading || !canLoadMoreChildren) return;
    const activeClient = requireClient();
    childLoading = true;
    error = null;
    try {
      const page = await activeClient.api.getChildren({
        scanId: session.scanId,
        nodeId: slice.focusNode.id,
        offset: childPageOffset,
        limit: rightPanelPageSize,
        sort: "size",
      });
      childItems = [...childItems, ...page.items];
      childPageTotal = page.total;
      childPageOffset = page.offset + page.items.length;
    } catch (cause) {
      error =
        cause instanceof Error ? cause.message : "Unable to load more folders";
    } finally {
      childLoading = false;
    }
  }

  function goBack() {
    const previous = breadcrumbs.at(-2);
    if (previous) void openNode(previous);
  }

  function addToCollector(node: TreeNodeSummary) {
    if (!session) return;
    const alreadyCovered = collectorEntries.some(
      (entry) =>
        node.path === entry.path || node.path.startsWith(`${entry.path}/`),
    );
    if (alreadyCovered) return;
    const kept = collectorEntries.filter(
      (entry) => !entry.path.startsWith(`${node.path}/`),
    );
    collectorEntries = [
      ...kept,
      {
        id: `${session.scanId}:${node.id}`,
        scanId: session.scanId,
        nodeId: node.id,
        path: node.path,
        name: node.name,
        size: node.size,
        addedAt: new Date().toISOString(),
      },
    ];
    contextMenu = null;
  }

  function removeFromCollector(id: string) {
    collectorEntries = collectorEntries.filter((entry) => entry.id !== id);
  }

  function openContextMenu(node: TreeNodeSummary, x: number, y: number) {
    contextMenu = { node, x, y };
  }

  async function showInFileManager(path: string) {
    const activeClient = requireClient();
    contextMenu = null;
    await activeClient.api.showInFileManager?.(path);
  }

  async function openInTerminal(path: string) {
    const activeClient = requireClient();
    contextMenu = null;
    await activeClient.api.openInTerminal?.(path);
  }

  async function cancelScan() {
    if (!session) return;
    const activeClient = requireClient();
    await activeClient.api.cancelScan(session.scanId);
    status = await activeClient.api.getScanStatus(session.scanId);
  }

  async function deleteCollected() {
    if (!session || collectorEntries.length === 0) return;
    const activeClient = requireClient();
    deleting = true;
    error = null;
    try {
      const outcome = await activeClient.api.executeCleanup({
        scanId: session.scanId,
        entries: collectorEntries,
      });
      if (outcome.errors.length > 0) {
        error = outcome.errors.join("\n");
        return;
      }
      collectorEntries = [];
      collectorOpen = false;
      status = {
        ...(status ?? emptyStatus(session.scanId)),
        state: "ready",
        message: `Removed ${formatBytes(outcome.bytesRemoved)}`,
        progress: 1,
      };
    } catch (cause) {
      error = cause instanceof Error ? cause.message : "Delete failed";
    } finally {
      deleting = false;
    }
  }

  function emptyStatus(scanId: string): ScanStatus {
    return {
      scanId,
      state: "idle",
      message: "Ready",
      progress: null,
      currentPath: null,
      bytesScanned: 0,
      entriesScanned: 0,
      rootIds: [],
      label: null,
      updatedAt: new Date().toISOString(),
    };
  }

  function sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  function findTreeNode(
    node: TreeSlice["tree"],
    nodeId: string,
  ): TreeNodeSummary | null {
    if (node.id === nodeId) return node;
    for (const child of node.children) {
      const found = findTreeNode(child, nodeId);
      if (found) return found;
    }
    return null;
  }
</script>

<svelte:head>
  <title>Space Lens</title>
</svelte:head>

{#if initializing}
  <main
    class="grid h-dvh place-items-center bg-background/70 text-foreground backdrop-blur-2xl"
  >
    <p class="text-sm text-muted-foreground">Starting Space Lens...</p>
  </main>
{:else if !session || !slice}
  <ScanPicker
    targets={scanTargets}
    mode={client?.mode ?? "demo"}
    {busy}
    {error}
    {status}
    onScan={startScan}
    onCancel={cancelScan}
    onForget={forgetScanTarget}
  />
{:else}
  <main
    class="grid h-dvh min-h-0 grid-rows-[auto_1fr_auto] overflow-hidden bg-background/70 text-foreground backdrop-blur-2xl"
    onpointerdown={(event) => {
      if (
        event.target instanceof HTMLElement &&
        event.target.closest(".context-menu")
      )
        return;
      contextMenu = null;
    }}
  >
    <header
      class={[
        "border-b bg-background/55 px-4 py-2 backdrop-blur-xl [-webkit-app-region:drag]",
        isKunkunMode ? "pl-24" : "",
      ]}
    >
      <div class="grid gap-1.5">
        <div class="flex min-h-8 items-center justify-between gap-3">
          <div class="flex min-w-0 items-center gap-2.5">
            <div
              class="grid size-7 shrink-0 place-items-center rounded-md bg-primary text-primary-foreground"
            >
              <FolderSearch class="size-3.5" />
            </div>
            <div class="flex min-w-0 items-center gap-2">
              <h1 class="truncate text-sm font-semibold leading-tight">
                Space Lens
              </h1>
              <Badge variant="outline" class="h-6 px-2 text-xs">
                {client?.mode ?? "demo"}
              </Badge>
            </div>
          </div>
          <div
            class="flex shrink-0 items-center gap-1.5 [-webkit-app-region:no-drag]"
          >
            <Button
              variant="ghost"
              size="sm"
              type="button"
              onclick={() => (collectorOpen = true)}
              class="px-2"
            >
              <PackageOpen class="size-4" />
              Collector
            </Button>
            <Button
              variant="secondary"
              size="sm"
              type="button"
              onclick={returnToScanPicker}
              class="px-2.5"
            >
              <RotateCcw class="size-4" />
              New scan
            </Button>
          </div>
        </div>
        <div class="min-w-0 [-webkit-app-region:no-drag]">
          <BreadcrumbBar
            items={breadcrumbs}
            onSelect={openNode}
            onBack={goBack}
            canGoBack={breadcrumbs.length > 1}
          />
        </div>
      </div>
    </header>

    <section
      class="grid min-h-0 grid-cols-[380px_minmax(0,1fr)] overflow-hidden"
    >
      <aside
        class="flex min-h-0 flex-col overflow-hidden border-r bg-sidebar/55"
      >
        <div class="border-b p-3">
          <div class="flex items-start justify-between gap-3">
            <div class="min-w-0">
              <h2 class="truncate text-sm font-semibold">
                {slice.focusNode.name}
              </h2>
              <p class="truncate text-xs text-muted-foreground">
                {currentChildren.length} of {childPageTotal} children
              </p>
            </div>
            <Badge variant="outline" class="tabular-nums">
              {formatBytes(slice.focusNode.size)}
            </Badge>
          </div>
        </div>
        <div class="flex min-h-0 flex-1 flex-col p-3">
          <ChildList
            items={currentChildren}
            {hoveredId}
            {collectedIds}
            onHover={(id) => (hoveredId = id)}
            onOpen={openNode}
            onCollect={addToCollector}
            onContext={openContextMenu}
          />
          {#if slice.omittedCount > 0}
            <p
              class="mt-3 rounded-md border border-dashed bg-background/40 px-3 py-2 text-xs text-muted-foreground"
            >
              {slice.omittedCount} more children omitted from chart slice ({formatBytes(
                slice.omittedBytes,
              )})
            </p>
          {/if}
          {#if canLoadMoreChildren}
            <Button
              class="mt-3 w-full"
              variant="outline"
              size="sm"
              type="button"
              onclick={loadMoreChildren}
              disabled={childLoading}
            >
              {childLoading ? "Loading" : "Load more"}
            </Button>
          {/if}
        </div>
      </aside>

      <section class="flex min-h-0 flex-col overflow-hidden bg-background/45">
        <div class="flex items-start justify-between gap-4 border-b p-3">
          <div class="min-w-0">
            <div class="flex items-center gap-2">
              <h2 class="truncate text-sm font-semibold">
                {chartInfo?.name ?? "Lens map"}
              </h2>
              {#if hoverInfo}
                <Badge variant="outline">hover</Badge>
              {/if}
            </div>
            <p class="mt-1 truncate font-mono text-xs text-muted-foreground">
              {chartInfo?.path ?? "Visible-depth radial partition"}
            </p>
            {#if chartInfo}
              <p class="mt-1 text-xs text-muted-foreground">
                {chartInfo.childCount} children
                {chartInfo.truncated ? " · slice truncated" : ""}
              </p>
            {/if}
          </div>
          <Badge class="shrink-0 tabular-nums"
            >{formatBytes(chartInfo?.size ?? slice.totalSize)}</Badge
          >
        </div>
        <div class="grid min-h-0 flex-1 place-items-center p-4">
          <SunburstChart
            tree={slice.tree}
            focusNode={slice.focusNode}
            {hoveredId}
            {collectedIds}
            onHover={(id) => (hoveredId = id)}
            onOpen={openNode}
            onContext={openContextMenu}
          />
        </div>
      </section>
    </section>

    {#if error}
      <div class="absolute bottom-16 left-4 right-4">
        <p
          class="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive"
        >
          {error}
        </p>
      </div>
    {/if}

    <StatusBar
      {status}
      {collectorTotal}
      collectorCount={collectorEntries.length}
      onOpenCollector={() => (collectorOpen = true)}
      onCancel={cancelScan}
    />

    {#if contextMenu}
      <div
        class="context-menu"
        style={`left: ${contextMenu.x}px; top: ${contextMenu.y}px`}
        role="menu"
      >
        <button
          type="button"
          onclick={() => openNode(contextMenu?.node.id ?? "")}>Expand</button
        >
        <button
          type="button"
          onclick={() => contextMenu && addToCollector(contextMenu.node)}
          >Move to Collector</button
        >
        <button
          type="button"
          disabled={!client?.api.showInFileManager}
          onclick={() => showInFileManager(contextMenu?.node.path ?? "")}
        >
          Show in Finder
        </button>
        <button
          type="button"
          disabled={!client?.api.openInTerminal}
          onclick={() => openInTerminal(contextMenu?.node.path ?? "")}
        >
          Open in Terminal
        </button>
      </div>
    {/if}

    <CollectorPanel
      open={collectorOpen}
      entries={collectorEntries}
      totalSize={collectorTotal}
      {deleting}
      onClose={() => (collectorOpen = false)}
      onRemove={removeFromCollector}
      onDelete={deleteCollected}
    />
  </main>
{/if}
