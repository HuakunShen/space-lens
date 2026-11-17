/** Deterministic, fictional data for the public static site. No user filesystem data. */
import type { MemorySnapshot, TreeSliceNode } from '@space-lens/contract'

const GiB = 1024 ** 3

function entry(id: string, name: string, sizeGiB: number, children: TreeSliceNode[] = []): TreeSliceNode {
  const size = Math.round(sizeGiB * GiB)
  return {
    id, name, path: `/demo/${id.replaceAll(':', '/')}`, size, logicalSize: size,
    depth: id.split(':').length - 1,
    ignored: false, collapsed: false, hasChildren: children.length > 0,
    childCount: children.length, isDirectory: children.length > 0,
    scanState: 'complete', skipReason: null, children,
    omittedBytes: 0, omittedCount: 0,
  }
}

export function makeDemoTree(): TreeSliceNode {
  return entry('workspace', 'Macintosh HD', 135, [
    entry('workspace:developer', 'Developer', 44, [
      entry('workspace:developer:node', 'node_modules', 21),
      entry('workspace:developer:rust', 'Rust builds', 16),
      entry('workspace:developer:docker', 'Docker images', 7),
    ]),
    entry('workspace:apps', 'Applications', 32, [
      entry('workspace:apps:xcode', 'Xcode', 18),
      entry('workspace:apps:browsers', 'Browsers', 8),
      entry('workspace:apps:tools', 'Utilities', 6),
    ]),
    entry('workspace:media', 'Media', 27, [
      entry('workspace:media:photos', 'Photos Library', 16),
      entry('workspace:media:videos', 'Video projects', 11),
    ]),
    entry('workspace:downloads', 'Downloads', 22, [
      entry('workspace:downloads:archives', 'Archives', 12),
      entry('workspace:downloads:installers', 'Installers', 7),
      entry('workspace:downloads:misc', 'Other files', 3),
    ]),
    entry('workspace:system', 'System Data', 10, [
      entry('workspace:system:caches', 'Caches', 6),
      entry('workspace:system:logs', 'Logs', 4),
    ]),
  ])
}

export function findDemoNode(root: TreeSliceNode, id: string): TreeSliceNode | null {
  if (root.id === id) return root
  for (const child of root.children) {
    const match = findDemoNode(child, id)
    if (match) return match
  }
  return null
}

export function makeDemoMemorySnapshot(): MemorySnapshot {
  const breakdown = {
    totalBytes: 32 * GiB,
    usedBytes: 22 * GiB,
    freeBytes: 10 * GiB,
    availableBytes: 11 * GiB,
    appBytes: 12 * GiB,
    wiredBytes: 4 * GiB,
    compressedBytes: 3 * GiB,
    cachedFilesBytes: 3 * GiB,
    swapTotalBytes: 8 * GiB,
    swapUsedBytes: 2 * GiB,
  }
  const examples = [
    { name: 'Code Editor', kind: 'app', footprint: 3.8, cpu: 12.4 },
    { name: 'Web Browser', kind: 'app', footprint: 2.6, cpu: 7.2 },
    { name: 'Container Engine', kind: 'agent', footprint: 1.9, cpu: 4.1 },
    { name: 'Design Studio', kind: 'app', footprint: 1.5, cpu: 2.0 },
    { name: 'System Compositor', kind: 'system', footprint: 0.6, cpu: 1.3 },
  ] as const
  const groups: MemorySnapshot['groups'] = examples.map(({ name, kind, footprint, cpu }, index) => ({
    groupId: `grp_${(index + 1).toString(16).padStart(16, '0')}`,
    name, bundleId: null, bundlePath: null, kind,
    mainPid: 1200 + index, pids: [1200 + index], cpuPercent: cpu,
    footprintBytes: Math.round(footprint * GiB), residentBytes: Math.round(footprint * GiB * 0.85),
    frontmost: index === 0, lastFrontmostAt: index === 0 ? '2026-11-17T13:00:00.000Z' : null,
    startedAt: '2026-11-17T08:00:00.000Z',
    protection: kind === 'system' ? 'systemComponent' : null,
  }))
  const processes: MemorySnapshot['processes'] = groups.map((group, index) => ({
    pid: group.mainPid, ppid: 1, startTimeUs: (index + 1) * 1_000_000,
    name: group.name, ownedByCurrentUser: true, groupId: group.groupId,
    kind: group.kind, cpuPercent: group.cpuPercent, footprintBytes: group.footprintBytes,
    residentBytes: group.residentBytes, virtualBytes: group.footprintBytes * 2,
    restricted: false, protection: group.protection, command: `${group.name.toLowerCase().replaceAll(' ', '-')}-demo`,
    user: 'sample-user', state: 'running', threads: 4 + index, listeningPorts: [],
  }))
  return {
    sequence: 1, sampledAt: '2026-11-17T13:00:00.000Z', intervalMs: 1_000,
    platform: 'macos',
    breakdown, pressure: { level: 'normal', source: 'kernel', availablePercent: 34.4 },
    groups, processes, notes: [],
  }
}
