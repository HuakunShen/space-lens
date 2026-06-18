#!/usr/bin/env node
import { existsSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { startWebServer } from './web-server.js'
import type { RunningWebServer } from './web-server.js'

interface WebCliOptions {
  hostname: string
  port: number
  staticDir: string
  apiOnly: boolean
  open: boolean
}

export function parseWebCliArgs(argv: readonly string[]): WebCliOptions {
  const options: WebCliOptions = {
    hostname: '127.0.0.1',
    port: 8757,
    staticDir: '',
    apiOnly: false,
    open: false,
  }

  for (let index = 2; index < argv.length; index += 1) {
    const arg = argv[index]
    const next = argv[index + 1]
    switch (arg) {
      case '--host':
      case '--hostname':
        if (!next) throw new Error(`${arg} requires a value`)
        options.hostname = next
        index += 1
        break
      case '--port':
        if (!next) throw new Error('--port requires a value')
        options.port = parsePort(next)
        index += 1
        break
      case '--static-dir':
        if (!next) throw new Error('--static-dir requires a value')
        options.staticDir = resolve(next)
        index += 1
        break
      case '--open':
        options.open = true
        break
      case '--api-only':
        options.apiOnly = true
        break
      case '--help':
      case '-h':
        printHelp()
        process.exit(0)
      default:
        throw new Error(`Unknown option: ${arg}`)
    }
  }

  if (!options.apiOnly) {
    options.staticDir ||= resolveStaticDir()
  }
  return options
}

export function runWebCli(argv: readonly string[] = process.argv): RunningWebServer | undefined {
  try {
    const options = parseWebCliArgs(argv)
    const running = startWebServer(options)
    running.server.on('error', (error) => {
      const message = error instanceof Error ? error.message : String(error)
      process.stderr.write(`spacelens-web: ${message}\n`)
      process.exitCode = 1
    })
    process.stdout.write(`Space Lens web listening on ${running.url}\n`)
    if (options.apiOnly) {
      process.stdout.write(`Serving authenticated KKRPC WebSocket API from ${running.rpcUrl}\n`)
    } else {
      process.stdout.write(`Open Space Lens: ${running.appUrl}\n`)
      process.stdout.write(`Serving static assets from ${options.staticDir}\n`)
    }
    if (options.open) {
      process.stdout.write('Open the URL above in your browser.\n')
    }
    return running
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    process.stderr.write(`spacelens-web: ${message}\n`)
    process.exitCode = 1
    return undefined
  }
}

function resolveStaticDir(): string {
  const fromEnv = process.env.SPACE_LENS_WEB_DIR
  const candidates = [
    fromEnv ? resolve(fromEnv) : '',
    resolve(dirname(fileURLToPath(import.meta.url)), 'web'),
    resolve(dirname(fileURLToPath(import.meta.url)), '../../web/build'),
    resolve(process.cwd(), '../web/build'),
    resolve(process.cwd(), 'apps/web/build'),
    resolve(process.cwd(), 'build'),
  ].filter(Boolean)

  const found = candidates.find((candidate) => existsSync(resolve(candidate, 'index.html')))
  if (found) return found

  throw new Error(
    `Unable to find built web assets. Run "yarn build:web" or pass --static-dir. Checked: ${candidates.join(', ')}`,
  )
}

function parsePort(raw: string): number {
  const value = Number(raw)
  if (!Number.isInteger(value) || value <= 0 || value > 65535) {
    throw new Error(`Invalid port: ${raw}`)
  }
  return value
}

function printHelp(): void {
  process.stdout.write(`Usage: spacelens-web [options]

Options:
  --host, --hostname <host>  Hostname to bind, defaults to 127.0.0.1
  --port <port>             Port to bind, defaults to 8757
  --static-dir <path>       Built apps/web directory to serve
  --api-only                Serve API routes only, for apps/web Vite dev server
  --open                    Print browser-open hint
  -h, --help                Show this help
`)
}

if (import.meta.url === `file://${process.argv[1]}`) {
  runWebCli()
}
