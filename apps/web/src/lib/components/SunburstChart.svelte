<script lang="ts">
  import type { TreeNodeSummary, TreeSliceNode } from "$lib/api/types";
  import { buildSunburstSegments } from "$lib/chart/sunburst";
  import { formatBytes } from "$lib/format";

  interface Props {
    tree: TreeSliceNode | null;
    focusNode: TreeNodeSummary | null;
    hoveredId: string | null;
    collectedIds: Set<string>;
    onHover: (id: string | null) => void;
    onOpen: (node: TreeNodeSummary) => void;
    onContext: (node: TreeNodeSummary, x: number, y: number) => void;
  }

  let {
    tree,
    focusNode,
    hoveredId,
    collectedIds,
    onHover,
    onOpen,
    onContext,
  }: Props = $props();
  const size = 620;
  const radius = 295;
  let segments = $derived(tree ? buildSunburstSegments(tree, radius) : []);
</script>

<section class="chart-wrap" aria-label="Disk usage chart">
  <svg
    class="sunburst"
    viewBox={`0 0 ${size} ${size}`}
    role="img"
    aria-label="Sunburst disk usage chart"
  >
    <g transform={`translate(${size / 2}, ${size / 2})`}>
      <circle class="center-well" r="64"></circle>
      {#each segments as segment (segment.id)}
        <path
          class="arc"
          class:hovered={hoveredId === segment.id}
          class:collected={collectedIds.has(segment.id)}
          d={segment.pathData}
          fill={segment.color}
          role="button"
          tabindex="0"
          aria-label={`${segment.name}, ${formatBytes(segment.size)}`}
          onmouseenter={() => onHover(segment.id)}
          onmouseleave={() => onHover(null)}
          onclick={() => onOpen(segment.node)}
          onkeydown={(event) => {
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              onOpen(segment.node);
            }
          }}
          oncontextmenu={(event) => {
            event.preventDefault();
            event.currentTarget.blur();
            onContext(segment.node, event.clientX, event.clientY);
          }}
        >
          <title>{segment.path} - {formatBytes(segment.size)}</title>
        </path>
      {/each}
      <text class="center-size" text-anchor="middle" y="-8"
        >{focusNode ? formatBytes(focusNode.size) : ""}</text
      >
      <text class="center-label" text-anchor="middle" y="24"
        >{focusNode?.name ?? ""}</text
      >
    </g>
  </svg>
</section>
