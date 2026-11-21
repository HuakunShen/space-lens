import { spawn } from 'node:child_process'
import {
  cpSync,
  existsSync,
  mkdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { homedir, platform } from 'node:os'
import { basename, dirname, join } from 'node:path'

export interface TrashPort {
  /** Trash every path. The returned map holds an error message per failed path; success maps to null. */
  trash(paths: readonly string[]): Promise<Map<string, string | null>>
}

function run(
  command: string,
  args: readonly string[],
  timeoutMs = 30_000,
): Promise<{ ok: boolean; stdout: string; stderr: string }> {
  return new Promise((resolve) => {
    const child = spawn(command, args, { stdio: ['ignore', 'pipe', 'pipe'] })
    let stdout = ''
    let stderr = ''
    const timer = setTimeout(() => {
      child.kill('SIGKILL')
    }, timeoutMs)
    child.stdout?.on('data', (chunk: Buffer) => {
      stdout = `${stdout}${chunk.toString()}`.slice(-8192)
    })
    child.stderr?.on('data', (chunk: Buffer) => {
      stderr = `${stderr}${chunk.toString()}`.slice(-2000)
    })
    child.on('error', (error) => {
      clearTimeout(timer)
      resolve({ ok: false, stdout, stderr: String(error) })
    })
    child.on('close', (code) => {
      clearTimeout(timer)
      resolve({ ok: code === 0, stdout, stderr })
    })
  })
}

function posixShellQuote(path: string): string {
  return path.replace(/\\/g, '\\\\').replace(/"/g, '\\"')
}

function darwinTrash(): TrashPort {
  return {
    async trash(paths) {
      const results = new Map<string, string | null>()
      if (paths.length === 0) return results
      const items = paths.map((path) => `POSIX file "${posixShellQuote(path)}"`).join(', ')
      // One Finder call per batch: asking once is also what keeps the
      // permission prompt from repeating for large plans.
      const script = `tell application "Finder" to delete {${items}}`
      const outcome = await run('osascript', ['-e', script])
      for (const path of paths) {
        results.set(path, outcome.ok ? null : `osascript failed: ${outcome.stderr.trim() || 'unknown error'}`)
      }
      return results
    },
  }
}

function linuxTrash(): TrashPort {
  return {
    async trash(paths) {
      const results = new Map<string, string | null>()
      if (paths.length === 0) return results
      // gio is the desktop-integrated mover; headless servers (and minimal
      // containers) do not ship it. The freedesktop layout below is what gio
      // itself writes, so the fallback is the same trash, not a downgrade.
      if (await hasGio()) {
        const outcome = await run('gio', ['trash', '--', ...paths])
        for (const path of paths) {
          results.set(
            path,
            outcome.ok ? null : `gio trash failed: ${outcome.stderr.trim() || 'unknown error'}`,
          )
        }
        return results
      }
      return freedesktopTrash(paths, homeTrashRoot())
    },
  }
}

let gioProbe: Promise<boolean> | null = null

function hasGio(): Promise<boolean> {
  gioProbe ??= run('gio', ['--version']).then((outcome) => outcome.ok)
  return gioProbe
}

/** `$XDG_DATA_HOME/Trash`, defaulting to `~/.local/share/Trash`. */
export function homeTrashRoot(): string {
  const dataHome = process.env.XDG_DATA_HOME
  const base = dataHome && dataHome.startsWith('/') ? dataHome : join(homedir(), '.local', 'share')
  return join(base, 'Trash')
}

/**
 * Moves every path into a freedesktop trash directory: `files/<name>` plus the
 * matching `info/<name>.trashinfo`. Never deletes — a cross-device source is
 * copied and then removed only after the copy lands, mirroring gio.
 */
export async function freedesktopTrash(
  paths: readonly string[],
  root: string,
): Promise<Map<string, string | null>> {
  const results = new Map<string, string | null>()
  const files = join(root, 'files')
  const info = join(root, 'info')
  mkdirSync(files, { recursive: true })
  mkdirSync(info, { recursive: true })
  for (const path of paths) {
    let isDirectory = false
    try {
      isDirectory = statSync(path).isDirectory()
    } catch {
      results.set(path, 'path vanished before it could be trashed')
      continue
    }
    try {
      const name = uniqueTrashedName(files, basename(path))
      const target = join(files, name)
      try {
        renameSync(path, target)
      } catch (error) {
        if ((error as NodeJS.ErrnoException | null)?.code !== 'EXDEV') throw error
        if (isDirectory) cpSync(path, target, { recursive: true })
        else cpSync(path, target)
        rmSync(path, { recursive: true })
      }
      writeFileSync(join(info, `${name}.trashinfo`), trashInfo(path))
      results.set(path, null)
    } catch (error) {
      results.set(
        path,
        `freedesktop trash failed: ${error instanceof Error ? error.message : String(error)}`,
      )
    }
  }
  return results
}

function uniqueTrashedName(filesDir: string, name: string): string {
  if (!existsSync(join(filesDir, name))) return name
  for (let counter = 2; ; counter += 1) {
    const candidate = `${name} ${counter}`
    if (!existsSync(join(filesDir, candidate))) return candidate
  }
}

/** `.trashinfo` body: the original location percent-encoded as a URI path. */
export function trashInfo(originalPath: string, deletedAt = new Date()): string {
  const encoded = encodeURIComponent(originalPath).replaceAll('%2F', '/')
  return `[Trash Info]\nPath=${encoded}\nDeletionDate=${deletedAt.toISOString().slice(0, 19)}\n`
}

function pwshString(value: string): string {
  return `'${value.replace(/'/g, "''")}'`
}

/**
 * Lists how many Recycle Bin entries currently display a given name.
 * Windows renames duplicates to "name (2)", so callers compare counts rather
 * than names when proving a delete landed in the bin.
 */
/**
 * The `-like` pattern that also matches Windows' duplicate rename
 * ("node_modules (2)"). Brackets are character classes in `-like`, so a name
 * containing them is escaped rather than silently matching other entries.
 */
export function recycleBinNamePattern(name: string): string {
  return `${name.replace(/[[\]]/g, (character) => `\`${character}`)} (*`
}

async function recycleBinCount(name: string): Promise<number | null> {
  const pattern = recycleBinNamePattern(name)
  const command =
    `Add-Type -AssemblyName Microsoft.VisualBasic; ` +
    `$shell = New-Object -ComObject Shell.Application; ` +
    `$hits = @($shell.Namespace(10).Items()) | Where-Object { $_.Name -eq ${pwshString(name)} -or $_.Name -like ${pwshString(pattern)} }; ` +
    `$hits = @($hits); Write-Output ("COUNT=" + $hits.Count)`
  const outcome = await run('powershell', ['-NoProfile', '-Command', command])
  if (!outcome.ok) return null
  const match = /COUNT=(\d+)/.exec(outcome.stdout)
  return match ? Number(match[1]) : null
}

/**
 * Recycle one path and prove it landed in the bin. Returns null on success or
 * an honest failure message; a permanent delete can never be reported as a
 * success this way, which is the whole point (see `windowsTrash`).
 */
async function recycleOne(path: string, isDirectory: boolean): Promise<string | null> {
  const name = basename(path)
  const before = await recycleBinCount(name)
  if (before === null) {
    return 'could not read the Recycle Bin to verify the move; refusing to delete'
  }
  const verb = isDirectory ? 'DeleteDirectory' : 'DeleteFile'
  const command =
    `Add-Type -AssemblyName Microsoft.VisualBasic; ` +
    `[Microsoft.VisualBasic.FileIO.FileSystem]::${verb}(${pwshString(path)}, 'OnlyErrorDialogs', 'SendToRecycleBin')`
  const outcome = await run('powershell', ['-NoProfile', '-Command', command])
  if (!outcome.ok) return `recycle failed: ${outcome.stderr.trim() || 'unknown error'}`
  const after = await recycleBinCount(name)
  if (after === null) {
    return 'the delete returned success but the Recycle Bin could not be read to confirm it'
  }
  if (after <= before) {
    // The documented fallback: the shell deletes permanently when it cannot
    // recycle (non-interactive session, quota, bin disabled).
    return 'the file was not found in the Recycle Bin afterwards, so it was deleted permanently'
  }
  return null
}

/**
 * A zero-byte probe proves the Recycle Bin works on a volume before user data
 * is touched. It is written beside the planned item's parent folder (same
 * volume, and a folder this user already owns), recycled, and checked.
 */
async function recycleBinWorksOn(probeDir: string): Promise<boolean> {
  const probe = join(probeDir, `.space-lens-trash-probe-${process.pid}-${Date.now()}`)
  try {
    writeFileSync(probe, '')
  } catch {
    return false
  }
  const failure = await recycleOne(probe, false)
  if (failure === null) return true
  // Keep the user's folder clean when the probe could not be recycled.
  try {
    rmSync(probe, { force: true })
  } catch {
    // Ignored: the probe is inert either way.
  }
  return false
}

function windowsTrash(): TrashPort {
  // Volume roots already probed in this process: the bin either works there
  // or the plan is refused, and that answer cannot change mid-run.
  const probed = new Map<string, boolean>()
  return {
    async trash(paths) {
      const results = new Map<string, string | null>()
      const volumeOf = (path: string) => {
        const match = /^([A-Za-z]:)[\\/]/.exec(path)
        return match ? `${match[1].toUpperCase()}\\` : '\\'
      }
      for (const path of paths) {
        let isDirectory = false
        try {
          isDirectory = statSync(path).isDirectory()
        } catch {
          results.set(path, 'path vanished before it could be trashed')
          continue
        }
        const volume = volumeOf(path)
        let works = probed.get(volume)
        if (works === undefined) {
          works = await recycleBinWorksOn(dirname(path))
          probed.set(volume, works)
        }
        if (!works) {
          results.set(
            path,
            `the Recycle Bin cannot recycle on ${volume} in this session; refusing to delete permanently`,
          )
          continue
        }
        results.set(path, await recycleOne(path, isDirectory))
      }
      return results
    },
  }
}

export function createSystemTrash(): TrashPort {
  switch (platform()) {
    case 'darwin':
      return darwinTrash()
    case 'linux':
      return linuxTrash()
    case 'win32':
      return windowsTrash()
    default:
      throw new Error(`no trash implementation for platform ${platform()}`)
  }
}
