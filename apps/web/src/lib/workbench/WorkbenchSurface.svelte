<script lang="ts">
  /** Shared 2D presentation for the legacy service and the opaque Xross view. */
  import { Search } from '@lucide/svelte'
  import { BreadcrumbBar, ChildList, CollectorPanel, StateBanner, StatusBar, SunburstChart } from '@space-lens/web-ui'
  import type { CollectorEntry, ScanStatus, TreeNodeSummary, TreeSlice } from '@space-lens/contract'
  import { useLensI18n } from '@space-lens/web-ui/i18n'

  interface Props {
    mode: 'browser' | 'desktop' | 'xross'
    status: ScanStatus
    slice: TreeSlice | null
    items: TreeNodeSummary[]
    hoveredId: string | null
    collector: CollectorEntry[]
    collectorOpen: boolean
    deleting: boolean
    error: string | null
    streamState?: 'connecting' | 'live' | 'reconnecting' | 'closed' | 'idle'
    onHover: (id: string | null) => void
    onFocus: (node: TreeNodeSummary) => void
    onCollect: (node: TreeNodeSummary) => void
    onCancel: () => void
    onOpenCollector: () => void
    onCloseCollector: () => void
    onRemoveCollector: (id: string) => void
    onDelete: () => void
    collectorActionLabel?: string
  }
  let { mode, status, slice, items, hoveredId, collector, collectorOpen, deleting, error,
    streamState = 'live', onHover, onFocus, onCollect, onCancel, onOpenCollector,
    onCloseCollector, onRemoveCollector, onDelete, collectorActionLabel }: Props = $props()
  const i18n = useLensI18n()
  const ancestors = $derived(slice === null ? [] : [...slice.ancestors, slice.focusNode])
  const collectedIds = $derived(new Set(collector.map((entry) => entry.nodeId)))
  const collectorTotal = $derived(collector.reduce((total, entry) => total + entry.size, 0))
</script>

<header data-tauri-drag-region class={['flex h-12 shrink-0 items-center justify-between border-b', mode === 'desktop' ? 'pl-[140px] pr-4' : 'px-4']}>
  <div class="flex items-center gap-2.5">
    <div class="grid size-8 place-items-center rounded-lg bg-primary text-primary-foreground"><Search size={15} /></div>
    <span class="font-semibold">Space Lens</span>
  </div>
  <span class="rounded-full border px-2 py-0.5 text-xs">{mode === 'xross' ? 'Xross' : mode}</span>
</header>
{#if error}<StateBanner state="error" title={i18n.t('xross.scan.failed')} detail={error} />{/if}
{#if slice?.truncated}
  <StateBanner state="truncated" title={i18n.t('xross.page.truncated')} detail={i18n.t('xross.slice.truncated', { count: i18n.count(slice.omittedCount) })} />
{/if}
{#if mode === 'browser' && streamState !== 'live'}
  <StateBanner state="disconnected"
    title={i18n.t(streamState === 'connecting' ? 'lens.stream.connecting' : streamState === 'reconnecting' ? 'lens.stream.reconnecting' : streamState === 'closed' ? 'lens.stream.closed' : 'lens.stream.idle')}
    detail={i18n.t('lens.stream.detail')} />
{/if}
<div class="grid flex-1 grid-cols-1 gap-4 lg:grid-cols-[minmax(0,1fr)_360px]">
  <div class="flex min-w-0 flex-col items-center gap-3">
    <SunburstChart tree={slice?.tree ?? null} focusNode={slice?.focusNode ?? null} {hoveredId} {collectedIds}
      {onHover} onOpen={onFocus} onContext={(node) => onCollect(node)} />
    <BreadcrumbBar items={ancestors} onSelect={onFocus} />
  </div>
  <ChildList {items} {hoveredId} {collectedIds} {onHover} onOpen={onFocus} onCollect={onCollect}
    onContext={(node) => onCollect(node)} />
</div>
<StatusBar {status} {collectorTotal} collectorCount={collector.length} {onOpenCollector} {onCancel} />
<CollectorPanel open={collectorOpen} entries={collector} totalSize={collectorTotal} {deleting}
  onClose={onCloseCollector} onRemove={onRemoveCollector} {onDelete} actionLabel={collectorActionLabel} />
