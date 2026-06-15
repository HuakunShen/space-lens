<script lang="ts">
  import { ModeWatcher } from "mode-watcher";
  import "./layout.css";
  import favicon from "$lib/assets/favicon.svg";

  let { children } = $props();
</script>

<svelte:head><link rel="icon" href={favicon} /></svelte:head>
<ModeWatcher
  defaultMode="dark"
  track={false}
  darkClassNames={["dark"]}
  modeStorageKey="space-lens-mode"
  themeStorageKey="space-lens-theme"
  themeColors={{ dark: "#171717", light: "#ffffff" }}
  disableTransitions={true}
/>
{@render children()}
