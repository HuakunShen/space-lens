import { spawn } from 'node:child_process'
import { lstatSync, realpathSync } from 'node:fs'
import { dirname, isAbsolute, resolve, sep } from 'node:path'

import type { HostPlatform, RevealAction } from '@space-lens/contract'

/**
 * Revealing a scanned path in the host's own file manager.
 *
 * The path is data, never a command line: it is resolved against the roots
 * this host serves and handed to the platform launcher as one argument (no
 * shell, no interpolation). Anything outside those roots is refused before a
 * process is spawned.
 */
export type { HostPlatform }

/** The host's own operating system — for the HTTP host that is the server. */
export function hostPlatform(): HostPlatform {
  if (process.platform === 'darwin') return 'macos'
  if (process.platform === 'win32') return 'windows'
  if (process.platform === 'linux') return 'linux'
  return 'unknown'
}

/** Component-aware containment: `/a/bc` does not live inside `/a/b`. */
function within(path: string, root: string): boolean {
  const base = root.endsWith(sep) ? root.slice(0, -1) : root
  if (base === '') return path.startsWith(sep)
  return path === base || path.startsWith(`${base}${sep}`)
}

/** Both the lexical path and its canonical form, against both root forms. */
function isServedPath(path: string, roots: readonly string[]): boolean {
  const candidates = new Set<string>([path])
  try {
    candidates.add(realpathSync(path))
  } catch {
    // A path that cannot be canonicalized is still judged by its lexical form.
  }
  for (const root of roots) {
    const rootForms = new Set<string>([root])
    try {
      rootForms.add(realpathSync(root))
    } catch {
      // An unreadable root only contributes its lexical form.
    }
    for (const candidate of candidates) {
      for (const rootForm of rootForms) {
        if (within(candidate, rootForm)) return true
      }
    }
  }
  return false
}

export interface FileManagerTarget {
  command: string
  args: string[]
}

/**
 * The platform's "show me this" launcher.
 *
 * `reveal` shows the item in its parent folder and selects it where the
 * platform can select; `open` opens a folder in the file manager itself.
 */
export function fileManagerTarget(
  platform: HostPlatform,
  path: string,
  isDirectory: boolean,
  action: RevealAction = 'reveal',
): FileManagerTarget {
  if (action === 'open' && !isDirectory) {
    throw new Error('only a folder can be opened in the file manager')
  }
  switch (platform) {
    case 'macos':
      return action === 'open' ? { command: 'open', args: [path] } : { command: 'open', args: ['-R', path] }
    case 'windows':
      return action === 'open'
        ? { command: 'explorer.exe', args: [path] }
        : { command: 'explorer.exe', args: [`/select,${path}`] }
    case 'linux':
      // No portable way to select a row on Linux: open the parent folder.
      return action === 'open' ? { command: 'xdg-open', args: [path] } : { command: 'xdg-open', args: [dirname(path)] }
    default:
      throw new Error('this platform has no file manager launcher')
  }
}

export type RevealFailure = 'invalid' | 'forbidden' | 'notFound' | 'unavailable'

export type RevealOutcome = { ok: true } | { ok: false; reason: RevealFailure; message: string }

/**
 * How a revealed path reaches the file manager. May be async (the platform
 * launcher resolves once the process is spawned).
 */
export type RevealLauncher = (target: FileManagerTarget) => void | Promise<void>

/**
 * The platform launcher: spawn the file manager and wait only until the
 * process exists. A missing `open`/`explorer`/`xdg-open` is reported to the
 * caller instead of being swallowed.
 */
const spawnFileManager: RevealLauncher = async (target) => {
  await new Promise<void>((resolve, reject) => {
    const child = spawn(target.command, target.args, { shell: false, stdio: 'ignore', detached: true })
    child.once('spawn', () => {
      child.unref()
      resolve()
    })
    child.once('error', reject)
  })
}

/**
 * Open `path` in the host's file manager.
 *
 * `roots` are the paths this host is allowed to serve; a path outside them is
 * refused (`forbidden`), never opened. `launch` is injectable so tests never
 * spawn a real file manager.
 */
export async function revealPath(
  path: string,
  roots: readonly string[],
  options: { platform?: HostPlatform; launch?: RevealLauncher; action?: RevealAction } = {},
): Promise<RevealOutcome> {
  if (path.trim() === '') return { ok: false, reason: 'invalid', message: 'a path is required' }
  if (!isAbsolute(path)) return { ok: false, reason: 'invalid', message: 'the path must be absolute' }
  const absolute = resolve(path)
  if (!isServedPath(absolute, roots)) {
    return {
      ok: false,
      reason: 'forbidden',
      message: 'this path is outside the roots this host serves',
    }
  }
  let isDirectory: boolean
  try {
    // `lstat`, not `stat`: a symlinked entry is revealed as the link the scan
    // reported, never as whatever it points at.
    isDirectory = lstatSync(absolute).isDirectory()
  } catch {
    return { ok: false, reason: 'notFound', message: `no such path: ${absolute}` }
  }
  const platform = options.platform ?? hostPlatform()
  const action = options.action ?? 'reveal'
  if (action === 'open' && !isDirectory) {
    return { ok: false, reason: 'invalid', message: 'only a folder can be opened in the file manager' }
  }
  let target: FileManagerTarget
  try {
    target = fileManagerTarget(platform, absolute, isDirectory, action)
  } catch (error) {
    return {
      ok: false,
      reason: 'unavailable',
      message: error instanceof Error ? error.message : 'this platform cannot open a file manager',
    }
  }
  try {
    await (options.launch ?? spawnFileManager)(target)
  } catch (error) {
    return {
      ok: false,
      reason: 'unavailable',
      message: error instanceof Error ? error.message : 'the file manager could not be started',
    }
  }
  return { ok: true }
}
