import { spawn, type ChildProcess } from 'node:child_process'
import * as vscode from 'vscode'

export interface Readiness {
  serviceInstanceId: string
  port: number
  url: string
}

export interface RunningService {
  readiness: Readiness
  ticket: string
  stop(): void
}

/**
 * Spawns `spacelens serve --machine`: loopback, port 0, no origin binding,
 * readiness JSON on stdout, single-use pairing ticket on stderr. The ticket is
 * exchanged host-side; the webview never sees it.
 */
export async function startMachineService(options: { cliPath: string; rootPath: string }): Promise<RunningService> {
  const child: ChildProcess = spawn(
    options.cliPath,
    ['serve', '--machine', '--port', '0', '--json', '--no-open', '--root', options.rootPath],
    { stdio: ['ignore', 'pipe', 'pipe'] },
  )

  const readiness = new Promise<Readiness>((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error('spacelens serve did not report readiness within 15s'))
    }, 15_000)
    let stdoutBuffer = ''
    child.stdout?.on('data', (chunk: Buffer) => {
      stdoutBuffer += chunk.toString()
      const line = stdoutBuffer.split('\n').find((candidate) => candidate.trimStart().startsWith('{'))
      if (line === undefined) return
      clearTimeout(timer)
      try {
        const parsed = JSON.parse(line) as Readiness & { ticketSingleUse?: boolean }
        if (typeof parsed.port !== 'number' || typeof parsed.url !== 'string') {
          throw new Error(`unexpected readiness shape: ${line}`)
        }
        resolve(parsed)
      } catch (error) {
        reject(error instanceof Error ? error : new Error(String(error)))
      }
    })
    child.on('exit', (code) => {
      clearTimeout(timer)
      reject(new Error(`spacelens serve exited early (code ${code}). Is "spacelens" installed and on PATH?`))
    })
  })

  const ticket = new Promise<string>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('spacelens serve printed no pairing ticket')), 15_000)
    let stderrBuffer = ''
    child.stderr?.on('data', (chunk: Buffer) => {
      stderrBuffer += chunk.toString()
      const marker = 'pairing URL (single use): '
      const line = stderrBuffer.split('\n').find((candidate) => candidate.includes(marker))
      if (line === undefined) return
      clearTimeout(timer)
      resolve(line.slice(line.indexOf(marker) + marker.length).trim())
    })
    child.on('exit', () => {
      clearTimeout(timer)
      reject(new Error('spacelens serve exited before printing a pairing ticket'))
    })
  })

  const [ready, pair] = await Promise.all([readiness, ticket])
  return {
    readiness: ready,
    ticket: pair,
    stop() {
      child.kill('SIGTERM')
    },
  }
}

export function rememberRoot(context: vscode.ExtensionContext, path: string): void {
  const previous = context.globalState.get<string[]>('spacelens.recentRoots') ?? []
  const next = [path, ...previous.filter((entry) => entry !== path)].slice(0, 5)
  void context.globalState.update('spacelens.recentRoots', next)
}

export function recentRoots(context: vscode.ExtensionContext): string[] {
  return context.globalState.get<string[]>('spacelens.recentRoots') ?? []
}
