<script lang="ts">
  import type { TreeNodeSummary, TreeSliceNode } from "../../types";
  import { buildSunburstSegments } from "../../lib/sunburst";
  import { useLensI18n } from "../../lib/i18n/context.svelte";

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
  const i18n = useLensI18n();
  let segments = $derived(tree ? buildSunburstSegments(tree, radius, i18n.t('lens.chart.other')) : []);
</script>

<section class="chart-wrap" aria-label={i18n.t('lens.chart.title')}>
  <svg
    class="sunburst"
    viewBox={`0 0 ${size} ${size}`}
    role="img"
    aria-label={i18n.t('lens.chart.title')}
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
          aria-label={i18n.t('lens.chart.arc', { name: segment.name, size: i18n.bytes(segment.size) })}
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
          <title>{segment.path} - {i18n.bytes(segment.size)}</title>
        </path>
      {/each}
      <text class="center-size" text-anchor="middle" y="-8"
        >{focusNode ? i18n.bytes(focusNode.size) : ""}</text
      >
      <text class="center-label" text-anchor="middle" y="24"
        >{focusNode?.name ?? ""}</text
      >
    </g>
  </svg>
</section>
