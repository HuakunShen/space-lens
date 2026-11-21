<script lang="ts">
  /**
   * Folder browser for the scan picker: the host lists one level at a time,
   * so a folder can be chosen without typing an absolute path — the way a
   * project picker in an editor works. Every row shows the entry's own size,
   * folders sort first, and the filter matches the current level.
   *
   * Keyboard: ↑/↓ move, Enter opens the highlighted folder (or follows a
   * typed absolute path), Backspace goes up, Esc closes.
   */
  import { ChevronRight, CornerLeftUp, File, Folder, LoaderCircle } from '@lucide/svelte'
  import type { FsEntry, FsListResponse } from '@space-lens/contract'
  import { formatBytes } from '../../lib/format'
  import { Button } from '../ui/button/index'
  import { Input } from '../ui/input/index'

  interface Props {
    /** Roots the host serves; the browser starts at the first one. */
    roots: string[]
    /** Folder to open first; falls back to the first root. */
    value?: string
    busy?: boolean
    /** One host request per drill; never polls. */
    listFolder: (path?: string) => Promise<FsListResponse>
    /** Accepts a folder (a row's Use button, or the footer action). */
    onSelect: (path: string) => void
    onClose?: () => void
    selectLabel?: string
  }

  let { roots, value = '', busy = false, listFolder, onSelect, onClose, selectLabel = 'Use this folder' }: Props =
    $props()

  let folder = $state<FsListResponse | null>(null)
  let loading = $state(false)
  let error = $state<string | null>(null)
  let filter = $state('')
  let active = $state(0)
  let listElement = $state<HTMLDivElement | null>(null)

  const SEPARATOR = $derived(folder?.path.includes('\\') ? '\\' : '/')

  function label(path: string): string {
    const parts = path.split(/[\\/]/).filter(Boolean)
    return parts[parts.length - 1] ?? path
  }

  /** Folders first (the host already sorts), then the filter, in order. */
  let rows = $derived.by(() => {
    const entries = folder?.entries ?? []
    const needle = filter.trim().toLowerCase()
    const visible = needle === '' ? entries : entries.filter((entry) => entry.name.toLowerCase().includes(needle))
    return visible
  })

  /** Breadcrumbs from the served root down to the open folder. */
  let crumbs = $derived.by(() => {
    if (!folder) return [] as { label: string; path: string }[]
    const root = [...roots].sort((left, right) => right.length - left.length).find((candidate) => {
      if (folder === null) return false
      return folder.path === candidate || folder.path.startsWith(candidate.endsWith(SEPARATOR) ? candidate : candidate + SEPARATOR)
    })
    if (root === undefined) return [{ label: folder.path, path: folder.path }]
    const trail: { label: string; path: string }[] = [{ label: label(root) || root, path: root }]
    const relative = folder.path === root ? '' : folder.path.slice(root.length + 1)
    let cursor = root
    for (const segment of relative.split(SEPARATOR).filter(Boolean)) {
      cursor = `${cursor.endsWith(SEPARATOR) ? cursor : cursor + SEPARATOR}${segment}`
      trail.push({ label: segment, path: cursor })
    }
    return trail
  })

  let typedPath = $derived(/^([/~]|[A-Za-z]:[\\/])/.test(filter.trim()))

  async function load(path: string | undefined): Promise<void> {
    loading = true
    error = null
    try {
      folder = await listFolder(path)
      filter = ''
      active = 0
      queueMicrotask(() => listElement?.focus())
    } catch (failure) {
      error = failure instanceof Error ? failure.message : String(failure)
    } finally {
      loading = false
    }
  }

  // Open where the caller already pointed, else the first served root. Keyed
  // on the initial value only: drilling must not be reset by a parent render.
  $effect(() => {
    const initial = value
    if (folder !== null || loading) return
    void load(initial === '' ? roots[0] : initial)
  })

  function move(delta: number): void {
    if (rows.length === 0) return
    active = Math.min(Math.max(active + delta, 0), rows.length - 1)
    queueMicrotask(() => document.getElementById(`fs-row-${active}`)?.scrollIntoView({ block: 'nearest' }))
  }

  function open(entry: FsEntry): void {
    if (entry.kind !== 'dir') return
    void load(entry.path)
  }

  function onKeydown(event: KeyboardEvent): void {
    // Typing in the filter owns the keyboard except for the commands below.
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      move(1)
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      move(-1)
    } else if (event.key === 'Enter') {
      event.preventDefault()
      const trimmed = filter.trim()
      if (typedPath && trimmed !== '') {
        void load(trimmed)
        return
      }
      const entry = rows[active]
      if (entry === undefined) return
      if (entry.kind === 'dir') void load(entry.path)
    } else if (event.key === 'Backspace' && filter === '') {
      event.preventDefault()
      if (folder?.parent) void load(folder.parent)
    } else if (event.key === 'Escape' && onClose) {
      event.preventDefault()
      onClose()
    }
  }
</script>

<section class="grid gap-3 rounded-xl border bg-card p-3" aria-label="Browse folders">
  <div class="flex items-center gap-1.5 text-[13px]" aria-label="Folders">
    {#each crumbs as crumb, index (crumb.path)}
      {#if index > 0}<ChevronRight class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />{/if}
      <button
        type="button"
        class="max-w-[14rem] truncate rounded px-1.5 py-0.5 hover:bg-muted"
        class:font-semibold={index === crumbs.length - 1}
        title={crumb.path}
        disabled={loading}
        onclick={() => void load(crumb.path)}>{crumb.label}</button
      >
    {/each}
    {#if folder?.parent}
      <Button
        variant="ghost"
        size="sm"
        class="ml-auto"
        title="Up one folder (Backspace)"
        disabled={loading}
        onclick={() => folder?.parent && void load(folder.parent)}
        ><CornerLeftUp size={14} /></Button
      >
    {/if}
  </div>

  <label class="relative flex items-center gap-2">
    <Input
      type="search"
      bind:value={filter}
      placeholder="Filter folders, or type an absolute path…"
      aria-label="Filter folders or enter a path"
      onkeydown={onKeydown}
    />
  </label>

  {#if error}
    <div class="rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive" role="alert">
      <p>{error}</p>
      <Button variant="outline" size="sm" class="mt-2" onclick={() => void load(folder?.path)}>Try again</Button>
    </div>
  {:else if loading && folder === null}
    <p class="flex items-center gap-2 p-3 text-sm text-muted-foreground" role="status">
      <LoaderCircle size={15} class="animate-spin" /> Reading folders…
    </p>
  {:else if folder}
    <div
      bind:this={listElement}
      class="max-h-72 min-h-24 overflow-auto rounded-lg border"
      class:opacity-60={loading}
      role="listbox"
      tabindex="0"
      aria-label={`Folders in ${folder.path}`}
      aria-activedescendant={rows.length === 0 ? undefined : `fs-row-${active}`}
      onkeydown={onKeydown}
    >
      {#if rows.length === 0}
        <p class="p-4 text-sm text-muted-foreground">
          {filter.trim() === '' ? 'No folders here.' : 'No folder matches that filter.'}
        </p>
      {:else}
        {#each rows as entry, index (entry.path)}
          <div
            id={`fs-row-${index}`}
            class="group flex items-center gap-2 px-2 py-1.5 text-[13px]"
            class:bg-muted={index === active}
            role="option"
            aria-selected={index === active}
            tabindex="-1"
            onmouseenter={() => (active = index)}
            onclick={() => open(entry)}
            onkeydown={onKeydown}
          >
            {#if entry.kind === 'dir'}
              <Folder size={15} class="shrink-0 text-primary" aria-hidden="true" />
            {:else}
              <File size={15} class="shrink-0 text-muted-foreground/50" aria-hidden="true" />
            {/if}
            <span class="min-w-0 flex-1 truncate" class:text-muted-foreground={entry.kind !== 'dir'}>{entry.name}</span>
            {#if entry.kind !== 'dir' && entry.size !== null}
              <!-- A directory's own length is its inode size (4 KiB everywhere
                   on ext4), which says nothing about the folder; only files
                   carry a meaningful entry size here. -->
              <span class="shrink-0 text-[11px] tabular-nums text-muted-foreground">{formatBytes(entry.size)}</span>
            {/if}
            {#if entry.kind === 'dir'}
              <Button
                variant="ghost"
                size="sm"
                class="shrink-0 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                title={`Choose ${entry.name}`}
                disabled={busy}
                onclick={(event) => {
                  event.stopPropagation()
                  onSelect(entry.path)
                }}>Use</Button
              >
            {/if}
          </div>
        {/each}
      {/if}
    </div>
    {#if folder.truncated}
      <p class="text-[11px] text-muted-foreground">
        Showing the first {folder.entries.length.toLocaleString()} entries — filter to narrow it down.
      </p>
    {/if}
    <div class="flex items-center justify-between gap-2 border-t pt-3">
      <p class="min-w-0 truncate font-mono text-[11px] text-muted-foreground" title={folder.path}>{folder.path}</p>
      <div class="flex shrink-0 gap-2">
        {#if onClose}<Button variant="outline" size="sm" onclick={onClose} disabled={busy}>Cancel</Button>{/if}
        <Button size="sm" disabled={busy || loading} onclick={() => folder && onSelect(folder.path)}
          >{selectLabel}</Button
        >
      </div>
    </div>
  {/if}
</section>
