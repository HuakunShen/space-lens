<script lang="ts">
  import { Copy, FolderOpen } from '@lucide/svelte'
  import type { AppAuditLocation } from '@space-lens/contract'
  import { formatBytes } from '../../lib/format'
  import { classificationBadge, classificationLabel } from '../../lib/app-audit'
  import { Checkbox } from '../ui/checkbox/index'

  interface Props {
    location: AppAuditLocation
    staged: boolean
    covered: boolean
    busy: boolean
    onToggle: (path: string) => void
    onCopy: (path: string) => void
    onReveal?: (path: string) => void
    /** Accepted but unwired this round: scanner handoff needs a scan-bound
     * lookup that does not exist yet, so no inspect button renders. */
    onInspect?: (path: string) => void
  }
  let { location, staged, covered, busy, onToggle, onCopy, onReveal }: Props = $props()

  let hint = $derived(
    location.classification === 'user-data' && location.purgeable
      ? 'Review / back up first'
      : !location.purgeable
        ? 'Not marked purgeable — removing may break the app or lose settings.'
        : null,
  )
  let name = $derived(location.label ?? location.path)
</script>

<li class="flex items-start gap-2 text-xs">
  <Checkbox
    checked={staged}
    disabled={covered || busy}
    onCheckedChange={() => onToggle(location.path)}
    aria-label={`Stage ${name} for cleanup review`}
    title={covered ? 'Included in a staged parent folder' : 'Stage for cleanup review'}
    class="mt-1"
  />
  {#if location.purgeable}<span
      class="mt-1.5 size-1.5 shrink-0 rounded-full bg-primary"
      title="Marked purgeable by classification"
    ></span>{/if}
  <div class="min-w-0 flex-1">
    <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
      <span class="font-semibold tabular-nums">{formatBytes(location.size)}</span>
      <span
        class={`rounded-full border px-2 py-0.5 text-[10px] font-medium text-muted-foreground ${classificationBadge(location.classification)}`}
        >{classificationLabel(location.classification)}</span
      >
      {#if location.label}<strong class="truncate">{location.label}</strong>{/if}
    </div>
    <div class="mt-0.5 flex min-w-0 items-center gap-1">
      <button
        type="button"
        class="min-w-0 flex-1 truncate text-left font-mono text-muted-foreground hover:text-foreground"
        title={`${location.path} — copy path`}
        onclick={() => onCopy(location.path)}>{location.path}</button
      >
      <button
        class="toolbar-icon"
        type="button"
        onclick={() => onCopy(location.path)}
        aria-label={`Copy path of ${name}`}
        title="Copy path"><Copy size={14} /></button
      >
      {#if onReveal}<button
          class="toolbar-icon"
          type="button"
          onclick={() => onReveal(location.path)}
          aria-label={`Reveal ${location.path} in Finder`}
          title="Reveal in Finder"><FolderOpen size={14} /></button
        >{/if}
    </div>
    {#if hint}<p class="mt-1 text-[11px] text-muted-foreground">{hint}</p>{/if}
    {#if location.comment}<details class="mt-1 text-muted-foreground">
        <summary class="cursor-pointer text-[11px]">Why / how to clean</summary>
        <p class="mt-1 text-[11px] leading-relaxed">{location.comment}</p>
      </details>{/if}
  </div>
</li>
