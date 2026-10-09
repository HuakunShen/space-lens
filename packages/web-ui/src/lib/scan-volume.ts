import type { ScanTarget, ScanVolume } from '../types.ts'

/** Compare host paths without depending on the browser's operating system. */
function normalizedPath(path: string): string | null {
  const windows = /^[a-z]:[\\/]/i.test(path) || /^[/\\]{2}[^/\\]/.test(path)
  const separated = windows ? path.replace(/\\/g, '/').toLowerCase() : path
  if (!windows && !separated.startsWith('/')) return null
  return separated.replace(/\/+$/, '') || '/'
}

/** Scan reports can identify the queried folder rather than its mount. */
type DisplayVolume = ScanVolume & { queriedPath?: boolean }

function containsPath(parent: string, child: string): boolean {
  return (
    child === parent ||
    (parent === '/' ? child.startsWith('/') && !child.startsWith('//') : child.startsWith(`${parent}/`))
  )
}

function containingVolume(focus: string, volumes: readonly ScanVolume[]): ScanVolume | null {
  const containing = volumes
    .map((volume) => ({ volume, mount: normalizedPath(volume.path) }))
    .filter(({ mount }) => mount !== null && containsPath(mount, focus))
    .sort((left, right) => (right.mount?.length ?? 0) - (left.mount?.length ?? 0))
  const deepest = containing[0]
  if (!deepest) return null
  return (
    containing.find(({ mount, volume }) => mount === deepest.mount && volumeCapacity(volume) !== null)?.volume ??
    deepest.volume
  )
}

/** Host volumes provide actual mount identity and current capacity; scan records are a fallback. */
export function resolveScanVolume(
  path: string | null | undefined,
  statusVolumes: readonly ScanVolume[],
  hostVolumes: readonly ScanVolume[],
): DisplayVolume | null {
  const focus = path ? normalizedPath(path) : null
  if (!focus) return null
  const host = containingVolume(focus, hostVolumes)
  if (host && volumeCapacity(host)) return host
  const scanned = containingVolume(focus, statusVolumes)
  if (!host) return scanned ? { ...scanned, queriedPath: true } : null
  // A parent mount's scan must never fill in an unknown child mount's capacity.
  const mount = normalizedPath(host.path)
  const queried = scanned ? normalizedPath(scanned.path) : null
  return scanned && volumeCapacity(scanned) && mount && queried && containsPath(mount, queried)
    ? { ...scanned, path: host.path }
    : host
}

export function scanVolumeLabel(volume: DisplayVolume, targets: readonly ScanTarget[]): string {
  const mount = normalizedPath(volume.path)
  const target = targets.find((target) => target.kind === 'volume' && normalizedPath(target.path) === mount)
  if (target?.label) return target.label
  if (mount === '/') return 'Filesystem'
  if (volume.queriedPath) return 'Disk containing this folder'
  return (
    volume.path
      .replace(/[\\/]+$/, '')
      .split(/[\\/]/)
      .at(-1) || volume.path
  )
}

export function volumeCapacity(
  volume: ScanVolume | null,
): { total: number; used: number; free: number; usedPercent: number } | null {
  if (!volume || !Number.isFinite(volume.totalBytes) || volume.totalBytes <= 0) return null
  const total = volume.totalBytes
  const validCount = (bytes: number) => Number.isFinite(bytes) && bytes >= 0 && bytes <= total
  // Zero free bytes is a full disk, not a missing measurement.
  const free = validCount(volume.freeBytes) ? volume.freeBytes : volume.availableBytes
  if (!validCount(free)) return null
  const used = total - free
  return { total, used, free, usedPercent: (used / total) * 100 }
}
