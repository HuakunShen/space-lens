import { spawn } from 'node:child_process'
import { dirname, resolve } from 'node:path'
import { existsSync } from 'node:fs'

import type { ServeOptions } from '@space-lens/host'
import { ConfigError, PortInUseError, ProblemError, startServe } from '@space-lens/host'

export class ServeUsageError extends Error {}

interface ParsedServeArgs extends ServeOptions {
  help?: boolean
  openExplicit?: boolean
}

const FLAG_ALIASES: Record<string, string> = {
  host: 'host',
  port: 'port',
  'allow-cidr': 'allowCidr',
  'allow-lan': 'allowLan',
  'allow-cleanup': 'allowCleanup',
  'ui-origin': 'uiOrigins',
  'ticket-ttl': 'ticketTtlSeconds',
  root: 'roots',
  'web-root': 'webRoot',
  json: 'json',
  machine: 'machine',
  'trust-proxy': 'trustProxy',
  open: 'open',
  'no-open': 'noOpen',
  help: 'help',
}

export function parseServeArgs(argv: readonly string[]): ParsedServeArgs {
  const parsed: ParsedServeArgs = {}
  let index = 0
  const nextValue = (flag: string): string => {
    const value = argv[index + 1]
    if (value === undefined) throw new ServeUsageError(`flag ${flag} needs a value`)
    index += 1
    return value
  }
  for (; index < argv.length; index += 1) {
    const raw = argv[index]
    if (!raw.startsWith('--'))
      throw new ServeUsageError(`unexpected argument: ${raw} (serve takes no positional paths; use --root)`)
    const name = raw.slice(2)
    const key = FLAG_ALIASES[name]
    if (key === undefined) throw new ServeUsageError(`unknown flag: --${name}`)
    switch (key) {
      case 'host':
        parsed.host = nextValue(raw)
        break
      case 'port':
        parsed.port = Number.parseInt(nextValue(raw), 10)
        if (!Number.isInteger(parsed.port)) throw new ServeUsageError('--port must be an integer')
        break
      case 'allowCidr':
        parsed.allowCidr = [...(parsed.allowCidr ?? []), nextValue(raw)]
        break
      case 'uiOrigins':
        parsed.uiOrigins = [...(parsed.uiOrigins ?? []), nextValue(raw)]
        break
      case 'roots':
        parsed.roots = [...(parsed.roots ?? []), resolve(nextValue(raw))]
        break
      case 'webRoot':
        parsed.webRoot = resolve(nextValue(raw))
        break
      case 'ticketTtlSeconds':
        parsed.ticketTtlSeconds = Number.parseInt(nextValue(raw), 10)
        if (!Number.isInteger(parsed.ticketTtlSeconds)) throw new ServeUsageError('--ticket-ttl must be an integer')
        break
      case 'allowLan':
      case 'allowCleanup':
      case 'json':
      case 'machine':
      case 'trustProxy':
      case 'help':
        parsed[key] = true
        break
      case 'open':
        parsed.open = true
        break
      case 'noOpen':
        parsed.open = false
        break
    }
  }
  return parsed
}

const HELP = `spacelens serve — serve the Space Lens web workbench over HTTP

Usage: spacelens serve [flags]

Bind and access:
  --host <bind>       loopback (default) | 0.0.0.0 | :: | an interface name | an IP
  --port <n>          default 9420; a busy default moves to a free port; --port 0 asks the OS
  --allow-cidr <cidr> client networks allowed to connect (repeatable); required for non-loopback binds
  --allow-lan         accept any client on a non-loopback bind (explicit opt-in)
  --trust-proxy       honor x-forwarded-for when checking client addresses

Capabilities:
  --allow-cleanup     grant paired sessions cleanup powers (trash-only, never permanent delete)
  --root <dir>        directories scans may target (repeatable; default: current directory)

Web UI:
  --ui-origin <url>   exact origins allowed to call this API cross-origin (repeatable)
  --open              open the pairing URL in a browser
  --web-root <dir>    serve this static build (defaults to the bundled apps/web build)

Auth:
  --ticket-ttl <s>    single-use pairing ticket lifetime (default 60)
  --machine           supervisor mode: loopback, port 0, no origin binding, readiness JSON on stdout,
                      pairing URL on stderr
  --json              machine-readable readiness line on stdout
  --trust-proxy       see above

A non-loopback --ui-origin requires ${'SPACLENS_HOSTED_PASSWORD'} in the environment.
Press p + Enter on the serving terminal to reprint the pairing URL.
`

async function openBrowser(url: string): Promise<void> {
  const command = process.platform === 'darwin' ? 'open' : process.platform === 'win32' ? 'cmd' : 'xdg-open'
  const args = process.platform === 'win32' ? ['/c', 'start', '', url] : [url]
  await new Promise<void>((resolve) => {
    const child = spawn(command, args, { stdio: 'ignore', detached: true })
    child.on('error', () => resolve())
    child.on('close', () => resolve())
    setTimeout(resolve, 2000).unref()
  })
}

export async function runServe(argv: readonly string[]): Promise<void> {
  const args = parseServeArgs(argv)
  if (args.help) {
    process.stdout.write(HELP)
    return
  }
  // Default web root: a `web/` directory beside the running CLI (the npm bin
  // layout produced by prepare-cli). No build → API-only host, said loudly.
  if (args.webRoot === undefined && process.argv[1] !== undefined) {
    const candidate = `${dirname(resolve(process.argv[1]))}/web`
    if (process.env.SPACLENS_DEBUG === '1') process.stderr.write(`webroot debug: argv1=${String(process.argv[1])} candidate=${candidate} exists=${existsSync(`${candidate}/200.html`)}` + '\n')
    if (existsSync(`${candidate}/200.html`)) args.webRoot = candidate
  }
  const server = await startServe(args)
  const pairingUrl = server.mintPairingUrl()

  const reprintPairing = (): void => {
    const fresh = server.mintPairingUrl()
    process.stderr.write(`pairing URL (single use): ${fresh}\n`)
  }

  if (args.json || args.machine) {
    process.stdout.write(
      `${JSON.stringify({
        serviceInstanceId: server.serviceInstanceId,
        port: server.port,
        url: server.baseUrl,
        ticketSingleUse: true,
      })}\n`,
    )
    process.stderr.write(`pairing URL (single use): ${pairingUrl}\n`)
  } else {
    process.stdout.write(
      [
        `Space Lens serve listening on ${server.baseUrl} (bound ${server.bind.display})`,
        `pairing URL (single use): ${pairingUrl}`,
        `scan roots: ${server.config.roots.join(', ')}`,
        `cleanup: ${server.config.allowCleanup ? 'enabled (trash only)' : 'disabled (read-only)'}`,
        args.uiOrigins?.length ? `cross-origin UIs: ${args.uiOrigins.join(', ')}` : '',
        `press p + Enter to reprint the pairing URL, Ctrl+C to stop`,
        '',
      ]
        .filter((line) => line !== '')
        .join('\n') + '\n',
    )
  }

  if (args.open) await openBrowser(server.baseUrl)

  if (!args.json && process.stdin.isTTY) {
    const readline = await import('node:readline')
    const lineInterface = readline.createInterface({ input: process.stdin, terminal: false })
    lineInterface.on('line', (line) => {
      if (line.trim().toLowerCase() === 'p' || line.trim().toLowerCase() === 'pair') reprintPairing()
    })
  }

  let stopping = false
  const shutdown = (): void => {
    if (stopping) return
    stopping = true
    void server.stop().then(() => process.exit(0))
  }
  process.on('SIGINT', shutdown)
  process.on('SIGTERM', shutdown)
}

export function reportServeError(error: unknown): void {
  if (error instanceof ServeUsageError) {
    process.stderr.write(`spacelens serve: ${error.message}\n\n${HELP}`)
    process.exitCode = 2
    return
  }
  if (error instanceof ConfigError) {
    process.stderr.write(`spacelens serve: ${error.message}\n`)
    process.exitCode = 2
    return
  }
  if (error instanceof PortInUseError) {
    process.stderr.write(`spacelens serve: ${error.message}\n`)
    process.exitCode = 1
    return
  }
  if (error instanceof ProblemError) {
    process.stderr.write(`spacelens serve: ${error.problem.code}: ${error.problem.message}\n`)
    process.exitCode = 1
    return
  }
  const message = error instanceof Error ? error.message : String(error)
  process.stderr.write(`spacelens serve: ${message}\n`)
  process.exitCode = 1
}
