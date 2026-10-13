import { spawn } from 'node:child_process'

import type { AppAuditRequest } from '@space-lens/contract'

const MAX_STDOUT_BYTES = 64 * 1024 * 1024
const MAX_STDERR_BYTES = 8192

export interface AppAuditSpawn {
  binary: string
  request: AppAuditRequest
  /** Host-configured home override; null inherits the process env (no --home flag). */
  homeOverride: string | null
}

export function appAuditArgs(request: AppAuditRequest, homeOverride: string | null): string[] {
  const args = [
    'app-audit',
    '--json',
    '--min-size',
    String(request.minSize),
    '--limit',
    String(request.limit),
    ...(request.includeHeuristic ? ['--all'] : ['--no-heuristic']),
  ]
  // The host passes --home ONLY from its own config override; otherwise the
  // CLI inherits HOME from the host process environment.
  if (homeOverride !== null) args.push('--home', homeOverride)
  return args
}

function runCli(binary: string, args: string[]): Promise<{ stdout: string; stderr: string }> {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, args, { shell: false, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true })
    let stdout = Buffer.alloc(0)
    let stderr = Buffer.alloc(0)
    let overflowed = false
    child.stdout.on('data', (chunk: Buffer) => {
      if (overflowed) return
      stdout = Buffer.concat([stdout, chunk])
      if (stdout.length > MAX_STDOUT_BYTES) {
        overflowed = true
        child.kill('SIGKILL')
      }
    })
    child.stderr.on('data', (chunk: Buffer) => {
      stderr = Buffer.concat([stderr, chunk]).subarray(-MAX_STDERR_BYTES)
    })
    child.on('error', (error) => {
      reject(new Error(`app CLI could not start: ${error.message}`))
    })
    child.on('close', (code, signal) => {
      if (overflowed) {
        reject(new Error('app CLI output exceeds 64 MiB protocol limit'))
        return
      }
      if (code !== 0 || signal !== null) {
        const detail = stderr.toString('utf8').trim()
        reject(new Error(detail === '' ? `app CLI exited (${signal ?? code})` : detail.slice(0, 1000)))
        return
      }
      resolve({ stdout: stdout.toString('utf8'), stderr: stderr.toString('utf8') })
    })
  })
}

export async function runAppAuditCli(spawnArgs: AppAuditSpawn): Promise<unknown> {
  const args = appAuditArgs(spawnArgs.request, spawnArgs.homeOverride)
  const { stdout } = await runCli(spawnArgs.binary, args)
  try {
    return JSON.parse(stdout)
  } catch (error) {
    throw new Error(
      `app CLI returned invalid JSON: ${error instanceof Error ? error.message.slice(0, 200) : String(error)}`,
    )
  }
}

export async function runAppIconCli(binary: string, appPath: string): Promise<{ dataUrl: string }> {
  const { stdout } = await runCli(binary, ['app-icon', '--path', appPath])
  let parsed: unknown
  try {
    parsed = JSON.parse(stdout)
  } catch (error) {
    throw new Error(
      `app CLI returned invalid JSON: ${error instanceof Error ? error.message.slice(0, 200) : String(error)}`,
    )
  }
  const dataUrl = (parsed as { dataUrl?: unknown } | null)?.dataUrl
  if (typeof dataUrl !== 'string' || dataUrl.length === 0) {
    throw new Error('app CLI returned no dataUrl')
  }
  return { dataUrl }
}
