<script lang="ts">
  // Decorative SVG folder material shared by chart labels; no host assets.
  let { x, y, size = 28 }: { x: number; y: number; size?: number } = $props()
</script>

<g class="chart-folder" transform={`translate(${x}, ${y}) scale(${size / 32})`} aria-hidden="true">
  <path class="folder-tab" d="M2 7a3 3 0 0 1 3-3h8l4 4h10a3 3 0 0 1 3 3v14H2Z" />
  <rect class="folder-front" x="2" y="10" width="28" height="19" rx="3" />
  <path class="folder-shine" d="M5 10.5h22" />
</g>
