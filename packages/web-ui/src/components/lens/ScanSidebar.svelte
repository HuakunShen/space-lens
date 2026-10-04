<script lang="ts">
  import { Folder, FolderPlus, HardDrive, X, ChartPie, FileSearch, Boxes, GitBranch } from '@lucide/svelte'
  import type { ScanTarget } from '../../types'

  interface Props {
    targets: ScanTarget[]
    view?: 'browse' | 'large-files' | 'caches' | 'gitignored'
    discoveryAvailable?: boolean
    gitignoredAvailable?: boolean
    onView?: (view: NonNullable<Props['view']>) => void
    selectedPath?: string | null
    customSelected?: boolean
    busy?: boolean
    onSelect: (target: ScanTarget) => void
    onCustom: () => void
    onForget?: (path: string) => Promise<void> | void
  }

  let { targets, view = 'browse', discoveryAvailable = false, gitignoredAvailable = discoveryAvailable, onView, selectedPath = null, customSelected = false, busy = false, onSelect, onCustom, onForget }: Props = $props()
  let forgetting = $state<string | null>(null)
  let recent = $derived(targets.filter(target => target.source === 'recent'))
  let locations = $derived(targets.filter(target => target.source !== 'recent'))

  async function forget(path: string) {
    if (!onForget) return
    forgetting = path
    try { await onForget(path) } finally { forgetting = null }
  }
</script>

<aside class="scan-sidebar" aria-label="Scan locations">
  <div class="sidebar-scroll">
    {#if onView}
      <h2>Workspace</h2>
      {#each [{ id: 'browse', label: 'Browse', icon: ChartPie }, {id: 'large-files', label: 'Large files', icon: FileSearch}, {id: 'caches', label: 'Developer cleanup', icon: Boxes}, {id: 'gitignored', label: 'Gitignored space', icon: GitBranch}] as option}
        <button class="sidebar-item" class:selected={view === option.id} type="button" disabled={option.id !== 'browse' && (!discoveryAvailable || option.id === 'gitignored' && !gitignoredAvailable)} aria-current={view === option.id ? 'page' : undefined}
          title={option.id === 'gitignored' && !gitignoredAvailable ? 'Scan a local project folder to classify its ignore rules' : option.id !== 'browse' && !discoveryAvailable ? 'Discovery is unavailable for this scan' : option.label}
          onclick={() => onView?.(option.id === 'large-files' ? 'large-files' : option.id === 'caches' ? 'caches' : option.id === 'gitignored' ? 'gitignored' : 'browse')}>
          <option.icon size={16} /><span>{option.label}</span>
        </button>
      {/each}
    {/if}
    <h2>Locations</h2>
    {#each locations as target (target.id)}
      <button class="sidebar-item" class:selected={!customSelected && selectedPath === target.path} type="button" disabled={busy}
        aria-pressed={!customSelected && selectedPath === target.path} onclick={() => onSelect(target)} title={target.path}>
        {#if target.kind === 'volume'}<HardDrive size={16} />{:else}<Folder size={16} />{/if}
        <span>{target.label}</span>
      </button>
    {/each}
    <button class="sidebar-item" class:selected={customSelected} type="button" disabled={busy} onclick={onCustom} aria-pressed={customSelected}>
      <FolderPlus size={16} /><span>New location</span>
    </button>
    {#if recent.length > 0}
      <h2>Recent Scans</h2>
      {#each recent as target (target.id)}
        <div class="sidebar-recent" class:selected={!customSelected && selectedPath === target.path}>
          <button class="sidebar-item" type="button" disabled={busy} onclick={() => onSelect(target)} title={target.path}
            aria-pressed={!customSelected && selectedPath === target.path}><Folder size={16} /><span>{target.label}</span></button>
          {#if onForget}
            <button class="sidebar-forget" type="button" disabled={busy || forgetting === target.path}
              onclick={() => void forget(target.path)} aria-label={`Forget ${target.label}`} title="Remove from recent scans"><X size={12} /></button>
          {/if}
        </div>
      {/each}
    {/if}
  </div>
  <span class="sidebar-footnote">Explore. Select. Review.</span>
</aside>
