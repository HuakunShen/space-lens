// Pre-scan volume capacity for the picker's drive cards. The scan walk
// reports volumes after it runs; the picker needs used/free before that,
// so capacity is read here with a single `df` call instead of a traversal.
import { execFile } from 'node:child_process'
import { platform } from 'node:os'

export interface VolumeCapacity {
  /** Filesystem totals for the volume holding the path. */
  totalBytes: number
  usedBytes: number
}

/** One mounted filesystem, in the contract's `ScanVolume` shape. */
export interface MountedVolume {
  path: string
  totalBytes: number
  availableBytes: number
  freeBytes: number
  isLocal: boolean
}

function runDf(args: readonly string[], timeoutMs = 10_000): Promise<string | null> {
  return new Promise((resolve) => {
    execFile('df', [...args], { timeout: timeoutMs, windowsHide: true }, (error, stdout) => {
      resolve(error ? null : stdout)
    })
  })
}

/**
 * Capacity for every distinct path, keyed by the input path. One `df` call
 * covers all paths; anything `df` cannot report (missing path, Windows,
 * parse failure) is simply absent, and the UI renders "unknown".
 *
 * `run` is injectable so tests can feed canned `df` output without spawning.
 */
export async function volumeCapacities(
  paths: readonly string[],
  run: (args: readonly string[]) => Promise<string | null> = runDf,
): Promise<Map<string, VolumeCapacity>> {
  const found = new Map<string, VolumeCapacity>()
  if (paths.length === 0 || platform() === 'win32') return found
  // `-k -P`: kilobyte blocks and one line per filesystem — the only two
  // output shapes POSIX guarantees, so Linux and macOS parse identically.
  const output = await run(['-k', '-P', '--', ...paths])
  if (output === null) return found
  const lines = output.split('\n')
  // Parse first, match second: a path belongs to the longest mount prefix
  // that contains it, so `/Volumes/Portable2TB` wins over `/` for anything
  // on the external disk. The filesystem root claims only what no deeper
  // mount claims — its stripped form is the empty string, which every
  // absolute path would otherwise start with.
  const entries: Array<{ mount: string; totalBytes: number; usedBytes: number }> = []
  for (let index = 1; index < lines.length; index += 1) {
    const fields = lines[index]?.trim().split(/\s+/) ?? []
    if (fields.length < 6) continue
    const totalKb = Number(fields[1])
    const usedKb = Number(fields[2])
    const mount = fields.slice(5).join(' ')
    if (!Number.isFinite(totalKb) || !Number.isFinite(usedKb) || totalKb <= 0) continue
    entries.push({ mount, totalBytes: totalKb * 1024, usedBytes: usedKb * 1024 })
  }
  // Longest mount first, so the first claim on a path is the deepest one.
  entries.sort((a, b) => b.mount.length - a.mount.length)
  for (const path of paths) {
    const hit = entries.find(
      (entry) => path === entry.mount || (entry.mount !== '/' && path.startsWith(`${entry.mount.replace(/\/+$/, '')}/`)),
    )
    // …with one exception: a path with no deeper mount lives on the root
    // filesystem, and the root line claims it.
    const root = hit ?? entries.find((entry) => entry.mount === '/')
    if (root) found.set(path, { totalBytes: root.totalBytes, usedBytes: root.usedBytes })
  }
  return found
}

/**
 * Every mounted local filesystem with its capacity: the "all my drives"
 * list for the pre-scan picker. Parses one bare `df -k -P` (no path args).
 *
 * APFS presents one physical disk as several mounts (`/`, `/System/Volumes/Data`,
 * preboot/VM siblings). Only `/` and `/Volumes/*` carry user-facing data, so
 * everything else is dropped, and the Data volume's numbers back the
 * Macintosh HD entry — `df /` alone understates a disk whose user data lives
 * on the Data twin.
 *
 * Display-only: mount points are facts, not scan targets — the picker offers
 * a scan only for paths inside the host's configured roots.
 */
export async function mountedVolumes(
  run: (args: readonly string[]) => Promise<string | null> = runDf,
): Promise<MountedVolume[]> {
  if (platform() === 'win32') return []
  const output = await run(['-k', '-P'])
  if (output === null) return []
  const parsed: MountedVolume[] = []
  const seen = new Set<string>()
  for (const line of output.split('\n').slice(1)) {
    const fields = line.trim().split(/\s+/)
    if (fields.length < 6) continue
    const [device, totalKb, , availableKb] = fields as [string, string, string, string]
    const mount = fields.slice(5).join(' ')
    const total = Number(totalKb)
    const available = Number(availableKb)
    if (!Number.isFinite(total) || !Number.isFinite(available) || total <= 0) continue
    if (seen.has(mount)) continue
    seen.add(mount)
    // Virtual filesystems and system-internal APFS siblings carry no
    // user-facing data; `/` and `/Volumes/*` are kept below.
    if (!mount.startsWith('/Volumes/') && mount !== '/') continue
    if (/^\/Volumes\/(Recovery|Preboot|Home|VM|Boot)$/i.test(mount)) continue
    parsed.push({
      path: mount,
      totalBytes: total * 1024,
      availableBytes: available * 1024,
      // `df -P` prints Available but not Free; available excludes the
      // root-reserved blocks, which is the honest "what can I still write".
      freeBytes: available * 1024,
      isLocal: true,
    })
  }
  // The startup volume's user data lives on its Data twin; prefer those
  // numbers for the Macintosh HD entry so "used" reflects the real disk.
  const data = parsed.find((volume) => volume.path === '/System/Volumes/Data')
  const root = parsed.find((volume) => volume.path === '/')
  if (root && data) root.availableBytes = data.availableBytes
  const volumes = parsed.filter((volume) => volume.path !== '/' || !data)
  if (root && data) {
    volumes.unshift({
      ...root,
      totalBytes: Math.max(root.totalBytes, data.totalBytes),
      availableBytes: data.availableBytes,
      freeBytes: data.freeBytes,
    })
  }
  // The startup volume first, then externals alphabetically — the order a
  // disk picker reads in.
  volumes.sort((a, b) => (a.path === '/' ? -1 : b.path === '/' ? 1 : a.path.localeCompare(b.path)))
  return volumes
}
