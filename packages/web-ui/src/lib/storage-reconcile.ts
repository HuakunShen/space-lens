// Turns a scan's APFS breakdown and skip counts into the rows of the scan
// details sheet: where the disk's "used" figure goes beyond what was measured,
// and what each kind of skip means. Pure, so the arithmetic is testable.
import { BY_DESIGN_SKIP_REASONS, type ScanCoverage, type StorageBreakdown } from '@space-lens/contract'

export interface ReconcileRow {
  key: string
  label: string
  detail: string
  bytes: number
}

export interface Reconciliation {
  inUseBytes: number
  capacityBytes: number
  measured: ReconcileRow
  /** Volumes in the container the scan never walks (VM, Preboot, Recovery, ...). */
  otherVolumes: ReconcileRow[]
  /** In use minus measured minus other volumes: snapshots, protected and unscanned folders. */
  unseen: ReconcileRow
  /** The scan plus other volumes exceeded "in use" (e.g. clones counted once per copy). */
  overcounted: boolean
  purgeableBytes: number | null
  /** The volumes a scan of the startup disk walks, for reference. */
  scannedVolumes: ReconcileRow[]
}

const VOLUME_DETAIL: Record<string, string> = {
  VM: 'Swap files and the sleep image, written when memory runs short.',
  Preboot: 'Boot files for each installed macOS.',
  Recovery: 'The recoveryOS used to reinstall or repair macOS.',
  Update: 'Staging space for macOS updates.',
  Hardware: 'Firmware data.',
  xART: 'Secure Enclave data.',
}

/** The volume a scan walks: Data on a startup disk, otherwise the largest one. */
function primaryDevice(storage: StorageBreakdown): string | undefined {
  return (storage.volumes.find((volume) => volume.role === 'Data') ?? storage.volumes[0])?.device
}

function oldestDate(stamp: string): string {
  return stamp.slice(0, 10)
}

export function reconcileStorage(storage: StorageBreakdown): Reconciliation {
  const primary = primaryDevice(storage)
  const walked = (volume: StorageBreakdown['volumes'][number]) =>
    volume.device === primary || volume.role === 'System'
  const otherVolumes = storage.volumes
    .filter((volume) => !walked(volume) && volume.inUseBytes > 0)
    .map((volume) => ({
      key: volume.device,
      label: volume.role === 'None' || volume.role === volume.name ? volume.name : `${volume.name} (${volume.role})`,
      detail: VOLUME_DETAIL[volume.role] ?? 'A separate volume in the same container; scan it as its own root.',
      bytes: volume.inUseBytes,
    }))
  const measuredBytes = Math.min(storage.scannedBytes, storage.inUseBytes)
  const otherBytes = otherVolumes.reduce((total, row) => total + row.bytes, 0)
  const remainder = storage.inUseBytes - measuredBytes - otherBytes
  const snapshots = storage.snapshots
  const unseenParts: string[] = []
  if (snapshots && snapshots.count > 0) {
    const oldest = snapshots.oldestTimeMachine
    unseenParts.push(
      `${snapshots.count.toLocaleString()} local snapshot${snapshots.count === 1 ? '' : 's'}` +
        (oldest ? ` (oldest ${oldestDate(oldest)}: files deleted since then still use space)` : ''),
    )
  }
  unseenParts.push('folders the scan could not read', 'folders outside the scanned roots', 'filesystem metadata')
  return {
    inUseBytes: storage.inUseBytes,
    capacityBytes: storage.capacityBytes,
    measured: {
      key: 'measured',
      label: 'Measured by this scan',
      detail: 'Allocated blocks of every file the scan walked.',
      bytes: measuredBytes,
    },
    otherVolumes,
    unseen: {
      key: 'unseen',
      label: 'Not seen by this scan',
      detail:
        `This covers ${unseenParts.join(', ')}.` +
        (snapshots && snapshots.count > 0 ? ' macOS does not report how much each snapshot holds.' : ''),
      bytes: Math.max(0, remainder),
    },
    overcounted: remainder < 0,
    purgeableBytes: storage.purgeableBytes,
    scannedVolumes: storage.volumes.filter(walked).map((volume) => ({
      key: volume.device,
      label: volume.role === 'System' ? `${volume.name} (sealed macOS)` : volume.name,
      detail: volume.role,
      bytes: volume.inUseBytes,
    })),
  }
}

export interface SkipReasonRow {
  reason: string
  label: string
  detail: string
  count: number
  /** False when the skip leaves no space unmeasured (see `BY_DESIGN_SKIP_REASONS`). */
  missing: boolean
}

const SKIP_REASONS: Record<string, { label: string; detail: string }> = {
  symlink: { label: 'Symbolic links', detail: 'Never followed by design; a link holds no file data.' },
  'permission-denied': {
    label: 'Access denied',
    detail: 'macOS refused to list these folders. Full Disk Access lets the scan read them.',
  },
  'mount-boundary': {
    label: 'Other volumes and mounts',
    detail: 'Separate filesystems; on APFS they appear as volume rows above.',
  },
  'separate-root': { label: 'Scanned as its own root', detail: 'Measured once, under its own root.' },
  'duplicate-directory': {
    label: 'Duplicate folder aliases',
    detail: 'The same folder reached by a second path, such as a firmlink; measured once.',
  },
  dataless: {
    label: 'Online-only iCloud files',
    detail: 'Not downloaded, so they use no local space. Downloads stay disabled during scans.',
  },
  'cloud-root': { label: 'Cloud storage folders', detail: 'Left to cloud inspection; their local size is unknown.' },
  'special-file': { label: 'Sockets and devices', detail: 'Not regular files; they hold no measurable data.' },
  'non-local-volume': { label: 'Network volumes', detail: 'Only local disks are scanned.' },
  'materialization-blocked': {
    label: 'Download blocked',
    detail: 'Reading would have downloaded the file, so it was left alone.',
  },
  missing: { label: 'Removed during the scan', detail: 'Gone before it could be measured.' },
  'io-error': { label: 'Read errors', detail: 'The filesystem returned an error.' },
  'gitignore-error': { label: 'Unreadable .gitignore', detail: 'The folder was still measured.' },
}

/** Skips by reason, largest first; empty for reports without the split. */
export function skipReasonRows(coverage: ScanCoverage): SkipReasonRow[] {
  return Object.entries(coverage.skippedByReason ?? {})
    .filter(([, count]) => count > 0)
    .map(([reason, count]) => {
      const known = SKIP_REASONS[reason]
      return {
        reason,
        label: known?.label ?? reason,
        detail: known?.detail ?? 'Left out of this scan.',
        count,
        missing: !BY_DESIGN_SKIP_REASONS.includes(reason),
      }
    })
    .sort((left, right) => right.count - left.count || left.reason.localeCompare(right.reason))
}
