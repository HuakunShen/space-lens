import { readdir, realpath, stat } from 'node:fs/promises'
import { homedir } from 'node:os'
import { dirname, isAbsolute, join, resolve, sep } from 'node:path'
import type { FsEntry, FsListRequest, FsListResponse } from '@space-lens/contract'
import { Problems } from './problems.ts'

/** How many entries a picker shows before it must narrow the filter. */
const DEFAULT_LIMIT = 1000
const MAX_LIMIT = 5000
/**
 * Above this many entries the folder is listed without per-entry sizes: a
 * picker must stay responsive on directories like a browser cache, and the
 * size of each entry is a nicety, not the point of choosing a folder.
 */
const STAT_BUDGET = 20000
/** Concurrent `stat` calls; enough to hide latency without a fd storm. */
const STAT_CONCURRENCY = 64

function within(path: string, root: string): boolean {
  return path === root || path.startsWith(root.endsWith(sep) ? root : `${root}${sep}`)
}

/** `~` means the host's home, as it does everywhere else in this host. */
export function expandHome(path: string): string {
  if (path === '~') return homedir()
  if (path.startsWith('~/') || path.startsWith('~\\')) return `${homedir()}${path.slice(1)}`
  return path
}

/**
 * Refuses a path outside the served roots. Containment is checked on the
 * real path, so a symlink inside a root cannot reach outside it — the same
 * rule scans apply, which keeps the picker from offering a folder the scan
 * would then reject. Returns the resolved path and the root that contains
 * it, so the caller can decide where the breadcrumb stops.
 */
async function containInRoots(
  path: string,
  roots: string[],
  realRoots: string[],
): Promise<{ resolved: string; root: string }> {
  let resolved: string
  try {
    resolved = await realpath(path)
  } catch {
    throw Problems.notFound(`folder does not exist: ${path}`)
  }
  for (const [index, realRoot] of realRoots.entries()) {
    if (within(resolved, realRoot)) return { resolved, root: roots[index] }
  }
  throw Problems.forbidden(`that folder is outside the locations this host serves (${roots.join(', ')})`)
}

async function entryFor(folder: string, name: string, kind: string, size: number | null): Promise<FsEntry> {
  return { name, path: join(folder, name), kind: kind === 'dir' || kind === 'file' ? kind : 'other', size }
}

/** `stat` a batch in parallel; a failed stat never fails the listing. */
async function measured(
  folder: string,
  entries: { name: string; type: 'dir' | 'file' | 'other' | 'link' | 'unknown' }[],
): Promise<FsEntry[]> {
  const output: FsEntry[] = new Array(entries.length)
  let cursor = 0
  const worker = async () => {
    for (;;) {
      const index = cursor++
      if (index >= entries.length) return
      const entry = entries[index]
      const full = join(folder, entry.name)
      try {
        const info = await stat(full)
        const kind = info.isDirectory() ? 'dir' : info.isFile() ? 'file' : 'other'
        output[index] = await entryFor(folder, entry.name, kind, info.size)
      } catch {
        // A broken symlink or a permission-denied entry: listed, not sized.
        output[index] = await entryFor(folder, entry.name, 'other', null)
      }
    }
  }
  await Promise.all(Array.from({ length: Math.min(STAT_CONCURRENCY, entries.length) }, worker))
  return output
}

/**
 * Lists one folder inside the served roots for the scan picker.
 *
 * Paths in the response stay *logical* (the path the caller asked for plus
 * the entry name) rather than canonical: on macOS a canonical `/Users/alice`
 * is `/System/Volumes/Data/Users/alice`, which is not what anyone wants to
 * read in a breadcrumb. Containment is still decided on the real path, so a
 * symlink entry that points outside the roots can be seen but not entered —
 * the next request refuses it.
 */
export async function listFolder(request: FsListRequest, configuredRoots: string[]): Promise<FsListResponse> {
  if (configuredRoots.length === 0) throw Problems.unavailable('this host serves no roots')
  const roots = configuredRoots.map((root) => resolve(root))
  const realRoots = await Promise.all(
    roots.map(async (root) => {
      try {
        return await realpath(root)
      } catch {
        return root
      }
    }),
  )
  const limit = Math.min(request.limit ?? DEFAULT_LIMIT, MAX_LIMIT)

  const requested = expandHome(request.path ?? roots[0])
  if (!isAbsolute(requested)) throw Problems.invalid('folder paths must be absolute')
  const folder = resolve(requested)
  const { resolved, root } = await containInRoots(folder, roots, realRoots)

  let dirents
  try {
    dirents = await readdir(folder, { withFileTypes: true })
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code
    if (code === 'ENOTDIR') throw Problems.invalid('that path is a file, not a folder')
    if (code === 'EACCES' || code === 'EPERM') throw Problems.forbidden('permission denied while reading that folder')
    throw Problems.invalid(`cannot read that folder: ${code ?? 'unknown error'}`)
  }

  const listed = dirents.map((dirent) => ({
    name: dirent.name,
    type: dirent.isDirectory()
      ? ('dir' as const)
      : dirent.isFile()
        ? ('file' as const)
        : dirent.isSymbolicLink()
          ? ('link' as const)
          : ('unknown' as const),
  }))

  // Directories first, then natural name order, so "10" follows "9" and a
  // filter keeps neighbours together.
  const withSize = listed.length <= STAT_BUDGET
  let entries: FsEntry[]
  if (withSize) {
    entries = await measured(folder, listed)
  } else {
    // A huge folder: keep types and skip sizes rather than stat 100k entries.
    entries = await Promise.all(
      listed.map(async (entry) => {
        if (entry.type !== 'link') return entryFor(folder, entry.name, entry.type, null)
        try {
          const info = await stat(join(folder, entry.name))
          return entryFor(folder, entry.name, info.isDirectory() ? 'dir' : info.isFile() ? 'file' : 'other', info.size)
        } catch {
          return entryFor(folder, entry.name, 'other', null)
        }
      }),
    )
  }

  entries.sort((left, right) => {
    if (left.kind !== right.kind) {
      if (left.kind === 'dir') return -1
      if (right.kind === 'dir') return 1
    }
    return left.name.localeCompare(right.name, undefined, { numeric: true, sensitivity: 'base' })
  })

  // The breadcrumb stops at the declared root: above it the host serves
  // nothing, so a "go up" there could only be refused. A root that is itself
  // a symlink keeps its logical spelling, and `resolved === realRoot` tells us
  // the folder *is* that root.
  const logicalParent = dirname(folder)
  const atRoot = resolved === realRoots[roots.indexOf(root)] || folder === root
  const parent = atRoot || logicalParent === folder ? null : logicalParent

  return {
    path: folder,
    parent,
    roots,
    entries: entries.slice(0, limit),
    truncated: entries.length > limit,
  }
}
