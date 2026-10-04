<script lang="ts">
  import { ShieldCheck, CircleAlert, ChevronRight } from '@lucide/svelte'
  import type { ScanStatus } from '../../types'
  import { formatBytes } from '../../lib/format'
  import { Button } from '../ui/button/index'
  import { Progress } from '../ui/progress/index'
  import * as Sheet from '../ui/sheet/index'

  let { status }: { status: ScanStatus } = $props()
  let open = $state(false)
  let coverage = $derived(status.coverage)
</script>

{#if coverage}
  <div class="scan-coverage-strip" aria-label="Local scan coverage">
    <span><ShieldCheck size={14} /> Local only</span>
    <span>{formatBytes(status.bytesScanned)} on disk</span>
    <span class="coverage-logical">{formatBytes(coverage.logicalBytes)} logical</span>
    <Button variant="ghost" size="sm" class="ml-auto h-7 gap-1.5 text-xs" onclick={() => (open = true)}>
      {#if coverage.skippedCount}<CircleAlert size={13} /> {coverage.skippedCount.toLocaleString()} skipped{:else}Scan details{/if}
      <ChevronRight size={13} />
    </Button>
  </div>
  <Sheet.Root bind:open>
    <Sheet.Content class="scan-coverage-panel">
      <Sheet.Header>
        <Sheet.Title>Scan details</Sheet.Title>
        <Sheet.Description>Measured local storage and the areas this scan left out.</Sheet.Description>
      </Sheet.Header>
      <div class="coverage-details">
        <div class="coverage-metrics">
          <div><span>Measured on disk</span><strong>{formatBytes(status.bytesScanned)}</strong></div>
          <div><span>Logical file size</span><strong>{formatBytes(coverage.logicalBytes)}</strong></div>
          <div><span>Files · folders</span><strong>{coverage.files.toLocaleString()} · {coverage.directories.toLocaleString()}</strong></div>
          <div><span>Scan duration</span><strong>{(coverage.elapsedMs / 1000).toLocaleString(undefined, { maximumFractionDigits: 1 })} s</strong></div>
        </div>
        <p>On-disk size counts allocated blocks. Logical size is the length of the measured files; sparse or compressed files can differ. Hard-link names each contribute logical length, while their allocated blocks are counted once. These are measurements, not a promise of space recovered by deleting.</p>
        {#if status.volumes?.length}
          <section aria-label="Disk capacity">
            <h3>Disk capacity</h3>
            {#each status.volumes as volume (volume.path)}
              <div class="coverage-volume">
                <strong>{volume.path === '/' ? coverage.protection === 'macos-no-materialization' ? 'Macintosh HD filesystem' : 'Filesystem' : volume.path === '/System/Volumes/Data' ? 'Macintosh HD · Data' : volume.path}</strong>
                <span>{formatBytes(volume.availableBytes)} available · {formatBytes(volume.totalBytes)} capacity</span>
                <Progress value={volume.totalBytes ? Math.max(0, Math.min(100, (1 - volume.availableBytes / volume.totalBytes) * 100)) : 0} />
              </div>
            {/each}
            <p>System capacity can include snapshots, filesystem overhead and storage outside the scan. APFS volumes share capacity; do not add their totals together.</p>
          </section>
        {/if}
        <section aria-label="Skipped scan areas">
          <h3>Not scanned</h3>
          <p>{coverage.skippedCount.toLocaleString()} areas skipped · {coverage.deniedCount.toLocaleString()} access denials. Cloud storage, online-only items and child mounts are left out. Their sizes are unknown. Symlinks are not followed; duplicate directory aliases are counted once.</p>
          {#if coverage.issuesTruncated}<p>Showing {coverage.issues.length.toLocaleString()} of {coverage.issueCount.toLocaleString()} notices.</p>{/if}
          <ul class="coverage-issues">
            {#each coverage.issues as issue, index (`${issue.path}:${index}`)}
              <li><strong>{issue.reason}</strong><span title={issue.path}>{issue.path}</span><p>{issue.message}</p></li>
            {:else}<li>No skipped areas were reported.</li>{/each}
          </ul>
        </section>
        <p class="coverage-protection">{coverage.protection === 'macos-no-materialization' ? 'macOS dataless downloads disabled for this scan.' : 'Local metadata scan.'} Cloud inspection is not included.</p>
      </div>
    </Sheet.Content>
  </Sheet.Root>
{/if}
