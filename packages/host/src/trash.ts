import { spawn } from 'node:child_process'
import { statSync } from 'node:fs'
import { platform } from 'node:os'

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
      const outcome = await run('gio', ['trash', '--', ...paths])
      for (const path of paths) {
        results.set(path, outcome.ok ? null : `gio trash failed: ${outcome.stderr.trim() || 'unknown error'}`)
      }
      return results
    },
  }
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
