<script lang="ts">
  /**
   * The Memory workbench sidebar: three real sections plus a live reading of
   * the snapshot the page is showing. This is the deliberate difference from
   * Storage's sidebar — there are no scan locations to list, so the space is
   * spent on navigation and the one number people keep looking at.
   */
  import type { MemorySnapshot } from '@space-lens/contract'
  import { ClockAlert, Gauge, History, ListTree, MemoryStick } from '@lucide/svelte'
  import Progress from '../ui/progress/progress.svelte'
  import { formatBytes, formatPercentValue } from '../../lib/memory/format.ts'
  import {
    MEMORY_SECTION_LABELS,
    MEMORY_SECTIONS,
    type MemorySection,
  } from '../../lib/memory/sections.ts'
  import { detectStaleProcesses } from '../../lib/memory/stale.ts'

  interface Props {
    section: MemorySection
    onSection: (section: MemorySection) => void
    snapshot: MemorySnapshot | null
    class?: string
  }

  let { section, onSection, snapshot, class: className = '' }: Props = $props()

  const SECTION_ICONS = {
    overview: Gauge,
    processes: ListTree,
    stale: ClockAlert,
    activity: History,
  } as const

  const PRESSURE_LABEL: Record<MemorySnapshot['pressure']['level'], string> = {
    normal: 'Normal',
    warning: 'Warning',
    critical: 'Critical',
    unknown: 'Unknown',
  }

  let usedShare = $derived.by(() => {
    if (snapshot === null || snapshot.breakdown.totalBytes <= 0) return 0
    return Math.min(100, (snapshot.breakdown.usedBytes / snapshot.breakdown.totalBytes) * 100)
  })
  let pressure = $derived(snapshot?.pressure.level ?? 'unknown')
  let staleCount = $derived(snapshot === null ? 0 : detectStaleProcesses(snapshot).length)
</script>

<aside class={['scan-sidebar memory-sidebar', className]} aria-label="Memory sections">
  <div class="sidebar-scroll">
    <h2 class="sidebar-heading">Memory</h2>
    {#each MEMORY_SECTIONS as item (item)}
      {@const Icon = SECTION_ICONS[item]}
      <button
        class="sidebar-item"
        class:selected={section === item}
        data-selected={section === item}
        type="button"
        aria-current={section === item ? 'page' : undefined}
        onclick={() => onSection(item)}
      >
        <Icon size={16} /><span>{MEMORY_SECTION_LABELS[item]}</span>
        {#if item === 'processes' && snapshot !== null}
          <span class="memory-sidebar-count">{snapshot.processes.length}</span>
        {:else if item === 'stale' && staleCount > 0}
          <span class="memory-sidebar-count memory-sidebar-count-warn">{staleCount}</span>
        {/if}
      </button>
    {/each}

    <h2 class="sidebar-heading">Now</h2>
    {#if snapshot === null}
      <p class="memory-sidebar-note">Reading memory…</p>
    {:else}
      <div class="memory-sidebar-card">
        <div class="memory-sidebar-card-row">
          <span>Memory used</span>
          <strong class="tabular-nums">{formatPercentValue(usedShare)}%</strong>
        </div>
        <Progress value={usedShare} max={100} class="h-1.5" aria-label="Memory used" />
        <p class="memory-sidebar-note">
          {formatBytes(snapshot.breakdown.usedBytes)} of {formatBytes(snapshot.breakdown.totalBytes)}
        </p>
        <div class="memory-sidebar-card-row">
          <span
            class={`memory-pressure ${pressure === 'normal'
              ? 'text-emerald-600 dark:text-emerald-400'
              : pressure === 'warning'
                ? 'text-amber-600 dark:text-amber-400'
                : pressure === 'critical'
                  ? 'text-destructive'
                  : 'text-muted-foreground'}`}
          >
            <MemoryStick size={12} />{PRESSURE_LABEL[pressure]}
          </span>
          <span class="memory-sidebar-time">{new Date(snapshot.sampledAt).toLocaleTimeString()}</span>
        </div>
        {#if snapshot.notes.includes('truncated')}
          <p class="memory-sidebar-note">The process list is truncated on this machine.</p>
        {/if}
      </div>
    {/if}
  </div>
  <p class="sidebar-footnote">Live memory reading, sampled while this view is open.</p>
</aside>
