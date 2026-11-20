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
import { basename, join } from 'node:path'

export interface TrashPort {
  /** Trash every path. The returned map holds an error message per failed path; success maps to null. */
  trash(paths: readonly string[]): Promise<Map<string, string | null>>
}

function run(command: string, args: readonly string[], timeoutMs = 30_000): Promise<{ ok: boolean; stderr: string }> {
  return new Promise((resolve) => {
    const child = spawn(command, args, { stdio: ['ignore', 'ignore', 'pipe'] })
    let stderr = ''
    const timer = setTimeout(() => {
      child.kill('SIGKILL')
    }, timeoutMs)
    child.stderr?.on('data', (chunk: Buffer) => {
      stderr = `${stderr}${chunk.toString()}`.slice(-2000)
    })
    child.on('error', (error) => {
      clearTimeout(timer)
      resolve({ ok: false, stderr: String(error) })
    })
    child.on('close', (code) => {
      clearTimeout(timer)
      resolve({ ok: code === 0, stderr })
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

function windowsTrash(): TrashPort {
  return {
    async trash(paths) {
      const results = new Map<string, string | null>()
      for (const path of paths) {
        let isDirectory = false
        try {
          isDirectory = statSync(path).isDirectory()
        } catch {
          results.set(path, 'path vanished before it could be trashed')
          continue
        }
        const verb = isDirectory ? 'DeleteDirectory' : 'DeleteFile'
        const command =
          `Add-Type -AssemblyName Microsoft.VisualBasic; ` +
          `[Microsoft.VisualBasic.FileIO.FileSystem]::${verb}('${path.replace(/'/g, "''")}', 'OnlyErrorDialogs', 'SendToRecycleBin')`
        const outcome = await run('powershell', ['-NoProfile', '-Command', command])
        results.set(path, outcome.ok ? null : `recycle failed: ${outcome.stderr.trim() || 'unknown error'}`)
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
