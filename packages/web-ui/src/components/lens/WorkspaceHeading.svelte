<script lang="ts">
  import { FolderPlus, Inbox, ChartPie } from '@lucide/svelte'
  import { Button } from '../ui/button/index'
  interface Props {
    title: string
    path?: string
    busy: boolean
    collectorCount: number
    chartVisible: boolean
    onNewScan: () => void
    onOpenCollector: () => void
    onToggleChart: () => void
  }
  let { title, path, busy, collectorCount, chartVisible, onNewScan, onOpenCollector, onToggleChart }: Props = $props()
</script>

<div class="hidden shrink-0 items-center justify-between gap-4 px-6 pb-5 windows:flex max-md:flex-wrap">
  <div class="min-w-0">
    <h1 class="truncate text-[28px] font-semibold leading-tight">{title}</h1>
    {#if path}<p class="mt-1 truncate text-xs text-muted-foreground" title={path}>{path}</p>{/if}
  </div>
  <div class="flex shrink-0 items-center gap-2">
    <Button onclick={onNewScan} disabled={busy}><FolderPlus size={16} />New scan</Button>
    <Button variant="outline" onclick={onOpenCollector} aria-label={`Review selection (${collectorCount})`}
      ><Inbox size={16} />Review ({collectorCount})</Button
    >
    <Button variant="outline" aria-label="Toggle storage map" aria-pressed={chartVisible} onclick={onToggleChart}
      ><ChartPie size={16} /><span class="max-lg:hidden">Storage map</span></Button
    >
  </div>
</div>
