<script lang="ts">
  import { FolderSearch, HardDrive } from '@lucide/svelte'
  import type { ScanTarget, ScanVolume } from '../../types'
  import { formatBytes } from '../../lib/format'

  interface Props {
    /** Mounted volumes with capacity, for the pre-scan drive list. */
    volumes: ScanVolume[]
    /** Configured scan roots: a drive offers Scan only inside these. */
    targets: ScanTarget[]
    busy?: boolean
    onScanDrive?: (path: string) => void
  }

  let { volumes, targets, busy = false, onScanDrive }: Props = $props()

  function driveName(path: string): string {
    if (path === '/') return 'Macintosh HD'
    const base = path.replace(/\/+$/, '').split('/').pop() ?? path
    return base || path
  }

  /** Whether a scan of this mount is servable: inside a configured root. */
  function scannable(path: string): boolean {
    return targets.some((target) => path === target.path || path.startsWith(`${target.path.replace(/\/+$/, '')}/`))
  }

  function usedOf(volume: ScanVolume): number {
    const free = volume.freeBytes || volume.availableBytes
    return Math.max(0, volume.totalBytes - free)
  }
</script>

{#if volumes.length > 0}
  <section class="drive-list" aria-label="Drives">
    <h2 class="drive-list-heading">Drives</h2>
    <ol>
      {#each volumes as volume (volume.path)}
        {@const used = usedOf(volume)}
        {@const free = volume.freeBytes || volume.availableBytes}
        {@const fraction = volume.totalBytes > 0 ? (used / volume.totalBytes) * 100 : 0}
        {@const canScan = scannable(volume.path)}
        <li class="drive-row">
          <span class="drive-icon"><HardDrive size={16} /></span>
          <div class="drive-meta">
            <div class="drive-top">
              <strong>{driveName(volume.path)}</strong>
              <span>{formatBytes(used)} of {formatBytes(volume.totalBytes)}</span>
            </div>
            <div
              class="drive-bar"
              role="img"
              aria-label={`${driveName(volume.path)}: ${formatBytes(used)} used of ${formatBytes(volume.totalBytes)}, ${formatBytes(free)} free`}
            >
              <span style={`width: ${fraction.toFixed(1)}%`}></span>
            </div>
            <div class="drive-sub">
              <span>{formatBytes(free)} free</span>
              {#if canScan && onScanDrive}
                <button type="button" class="drive-scan-btn" disabled={busy} onclick={() => onScanDrive(volume.path)}>
                  <FolderSearch size={13} />Scan
                </button>
              {:else}
                <span class="drive-nonscan">outside scan roots</span>
              {/if}
            </div>
          </div>
        </li>
      {/each}
    </ol>
  </section>
{/if}
