<script lang="ts">
  import { File, Folder, Copy, FolderOpen, Search, LoaderCircle, ArrowDown } from '@lucide/svelte'
  import type { DiscoveryItem, DiscoveryKind, DiscoveryPage, TreeNodeSummary } from '@space-lens/contract'
  import { formatBytes } from '../../lib/format'
  import LensSelect from './LensSelect.svelte'
  import { Button } from '../ui/button/index'
  import { Checkbox } from '../ui/checkbox/index'
  import { Input } from '../ui/input/index'
  interface Props {
    kind: DiscoveryKind
    page: DiscoveryPage | null
    items: DiscoveryItem[]
    loading: boolean
    error: string | null
    minSize: number
    disabled?: boolean
    isSelected: (node: TreeNodeSummary) => boolean
    isCovered: (node: TreeNodeSummary) => boolean
    onToggle: (node: TreeNodeSummary) => void
    onSelect: (nodes: TreeNodeSummary[]) => void
    onDeselect: (nodes: TreeNodeSummary[]) => void
    onMinSize: (value: number) => void
    onLoadMore: () => void
    onRetry: () => void
    onBrowse: (item: DiscoveryItem) => void
    onCopy: (path: string) => void
  }
  let { kind, page, items, loading, error, minSize, disabled = false, isSelected, isCovered,
    onToggle, onSelect, onDeselect, onMinSize, onLoadMore, onRetry, onBrowse, onCopy }: Props = $props()
  let query = $state('')
  let category = $state('all')
  let categories = $derived([...new Set(items.map(item => item.category))].sort())
  const sizeOptions = [{ value: '0', label: 'All sizes' }, { value: String(10 * 1024 ** 2), label: '10 MB+' }, { value: String(100 * 1024 ** 2), label: '100 MB+' }, { value: String(1024 ** 3), label: '1 GB+' }]
  let categoryOptions = $derived([{ value: 'all', label: 'All types' }, ...categories.map(name => ({ value: name, label: name }))])
  let visible = $derived(items.filter(item => (category === 'all' || item.category === category) && item.node.path.toLowerCase().includes(query.toLowerCase())))
  let title = $derived(kind === 'large-files' ? 'Large files' : kind === 'caches' ? 'Developer cleanup' : 'Gitignored space')
  let subtitle = $derived(kind === 'large-files' ? 'Largest files across this scan, including summarized folders.' : kind === 'caches' ? 'Recognized dependencies, build output and tool caches. Review before removing.' : 'Matched by .gitignore. This can include valuable untracked files; nothing is selected automatically.')
  function consequence(category: string): string {
    const name = category.toLowerCase()
    if (name.includes('dependenc')) return 'Dependencies · reinstall before use'
    if (name.includes('rust') || name.includes('build')) return 'Generated output · rebuild before use'
    if (name.includes('pytest')) return 'Test state · previous run hints will reset'
    if (kind === 'caches') return 'Tool cache · regenerated on next use'
    return category
  }
</script>
<section class="discovery-view" aria-label={title} aria-busy={loading}>
  <div class="discovery-heading">
    <div><span class="section-eyebrow">DISCOVER & REVIEW</span><h1>{title}</h1><p>{subtitle}</p></div>
    {#if page}<div class="discovery-total"><strong>{formatBytes(page.totalSize)}</strong><span>{page.total.toLocaleString()} {kind === 'large-files' ? 'file' : 'item'}{page.total === 1 ? '' : 's'} matched</span></div>{/if}
  </div>
  <div class="discovery-filters">
    <label class="search-field"><Search size={15} /><Input type="search" bind:value={query} placeholder="Search loaded results" aria-label="Search loaded results" /></label>
    {#if kind === 'caches'}<div class="filter-select"><span>Type</span><LensSelect value={category} options={categoryOptions} label="Cache type" class="min-w-36" onChange={value => (category = value)} /></div>{/if}
    <div class="filter-select"><span>Size</span><LensSelect value={String(minSize)} options={sizeOptions} label="Minimum size" class="min-w-28" onChange={value => onMinSize(Number(value))} /></div>
  </div>
  <div class="selection-tools">
    <span>{visible.length.toLocaleString()} shown{page && items.length < page.total ? ` · ${items.length.toLocaleString()} of ${page.total.toLocaleString()} loaded` : ''} · largest first</span>
    <div><button type="button" onclick={() => onSelect(visible.map(item => item.node))} disabled={disabled || visible.length === 0}>Select shown ({visible.length})</button><button type="button" onclick={() => onDeselect(visible.map(item => item.node))} disabled={disabled || !visible.some(item => isSelected(item.node) && !isCovered(item.node))}>Deselect shown</button></div>
  </div>
  {#if error}<div class="inline-error" role="alert"><p>{error}</p><Button variant="outline" size="sm" onclick={onRetry}>Try again</Button></div>{/if}
  <div class="discovery-rows">
    {#each visible as item (item.node.id)}
      <div class="discovery-row" class:selected={isSelected(item.node)}>
        <Checkbox checked={isSelected(item.node)} disabled={disabled || isCovered(item.node) || item.node.scanState === 'skipped' || item.node.scanState === 'partial'} onCheckedChange={() => onToggle(item.node)} aria-label={`Select ${item.node.path}`} title={isCovered(item.node) ? 'Included in a selected parent folder' : 'Add to review selection'} />
        <span class="file-kind">{#if item.isDirectory}<Folder size={19} />{:else}<File size={19} />{/if}</span>
        <div class="discovery-name"><strong>{item.node.name}</strong><span class="discovery-path" title={item.node.path}>{item.node.path}</span><span class="candidate-reason">{isCovered(item.node) ? 'Included in selected folder' : consequence(item.category)}</span></div>
        <span class="discovery-size">{formatBytes(item.node.size)}</span>
        <div class="row-actions"><button class="toolbar-icon" type="button" onclick={() => onCopy(item.node.path)} aria-label={`Copy path of ${item.node.name}`} title="Copy path"><Copy size={14} /></button><button class="toolbar-icon" type="button" onclick={() => onBrowse(item)} aria-label={`Show ${item.node.name} in browse`} title="Show containing folder"><FolderOpen size={15} /></button></div>
      </div>
    {:else}
      {#if !loading && !error}<div class="discovery-empty"><Search size={28} /><strong>{query || category !== 'all' ? 'No matching loaded items' : 'No items found at this size'}</strong><p>{query || category !== 'all' ? 'Try a different filter or load more results.' : 'Choose a smaller size or scan another location.'}</p></div>{/if}
    {/each}
    {#if loading}<div class="discovery-loading" role="status"><LoaderCircle size={18} class="animate-spin" /> Inspecting files and caches…</div>{/if}
    {#if page && items.length < page.total && !loading}<div class="load-more"><Button variant="outline" size="sm" onclick={onLoadMore}><ArrowDown size={14} /> Load more ({Math.min(200, page.total - items.length)})</Button></div>{/if}
  </div>
  <p class="discovery-footnote">Selection is shared across views. Gitignored space and cache groups may overlap; their totals are separate.</p>
</section>
