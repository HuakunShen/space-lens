import { randomBytes } from 'node:crypto'
import type { AddressInfo } from 'node:net'

import { serve, type ServerType } from '@hono/node-server'
import { API_MAJOR, CONTRACT_VERSION, healthPayload, type Capabilities } from '@space-lens/contract'

import { AuthService, hashHostedPassword } from './auth.ts'
import { createAssetHandler } from './assets.ts'
import { createClientAllowlist } from './cidr.ts'
import type { ResolvedServeConfig, ResolvedHost, ServeOptions } from './config.ts'
import { resolveServeConfig } from './config.ts'
import { EventRing } from './events.ts'
import { buildApp } from './app.ts'
import { ScanManager } from './scan-store.ts'
import { createSystemTrash } from './trash.ts'

export class PortInUseError extends Error {
  readonly port: number

  constructor(port: number) {
    super(`port ${port} is already in use`)
    this.port = port
  }
}

export interface RunningServer {
  port: number
  bind: ResolvedHost
  serviceInstanceId: string
  baseUrl: string
  capabilities: Capabilities
  events: EventRing
  scans: ScanManager
  auth: AuthService
  config: ResolvedServeConfig
  /** Origin bound into minted tickets ('' in machine mode). */
  ticketOrigin: string
  mintPairingUrl(): string
  stop(): Promise<void>
}

function listen(server: ServerType): Promise<void> {
  return new Promise((resolve, reject) => {
    const onError = (error: NodeJS.ErrnoException): void => {
      server.off('listening', onListening)
      reject(error)
    }
    const onListening = (): void => {
      server.off('error', onError)
      resolve()
    }
    server.once('error', onError)
    server.once('listening', onListening)
  })
}

export async function startServe(options: ServeOptions): Promise<RunningServer> {
  const config = resolveServeConfig(options)
  const serviceInstanceId = `inst_${randomBytes(6).toString('hex')}`
  const hostedPassword = config.hostedPassword === null ? null : hashHostedPassword(config.hostedPassword)
  const scopes = config.allowCleanup
    ? ['scan:read', 'scan:start', 'cleanup:plan', 'cleanup:execute']
    : ['scan:read', 'scan:start']

  const auth = new AuthService({
    serviceInstanceId,
    ticketTtlMs: config.ticketTtlMs,
    sessionTtlMs: config.sessionTtlMs,
    hostedPassword,
    scopes,
  })
  const events = new EventRing()
  const scans = new ScanManager({
    events,
    trash: options.trash ?? createSystemTrash(),
    roots: config.roots,
    cleanupMode: config.allowCleanup ? 'trash' : 'none',
    maxConcurrent: config.maxConcurrentScans,
    maxTotal: config.maxTotalScans,
  })
  const capabilities: Capabilities = {
    apiMajor: API_MAJOR,
    contractVersion: CONTRACT_VERSION,
    scan: { start: true, cancel: true, maxConcurrent: config.maxConcurrentScans },
    cleanup: { plan: config.allowCleanup, execute: config.allowCleanup, mode: config.allowCleanup ? 'trash' : 'none' },
    host: { folderPicker: false },
    icloud: 'unavailable',
  }

  const app = buildApp({
    config,
    auth,
    scans,
    events,
    assets: createAssetHandler(config.webRoot),
    health: healthPayload(serviceInstanceId),
    capabilities,
    allowlist: createClientAllowlist(config.allowCidr),
  })

  let server: ServerType | null = null
  let port = config.port
  let attempt = 0
  for (;;) {
    attempt += 1
    const candidate = serve({ fetch: app.fetch, port, hostname: config.bind.address })
    try {
      await listen(candidate)
      server = candidate
      break
    } catch (error) {
      candidate.close()
      const code = (error as NodeJS.ErrnoException).code
      if (code !== 'EADDRINUSE') throw error
      // A busy *default* port quietly moves to a free one and names both; an
      // explicitly requested busy port is refused so the operator notices.
      if (config.portExplicit || attempt >= 2) throw new PortInUseError(port)
      port = 0
    }
  }

  const boundPort = (server.address() as AddressInfo).port
  port = boundPort
  app.rebindGate(boundPort)

  const apiAuthority = displayAuthority(config, boundPort)
  const apiBase = `http://${apiAuthority}`
  // A ticket binds to the exact Origin header the spender will send: '' for a
  // machine supervisor (which sends no Origin at all), the deployed UI's
  // origin when one is configured, otherwise this listener's own origin.
  const ticketOrigin = config.machine ? '' : (config.uiOrigins[0] ?? apiBase)

  let stopped = false
  const stop = async (): Promise<void> => {
    if (stopped) return
    stopped = true
    for (const status of scans.list()) {
      if (status.state === 'scanning') scans.cancel(status.scanId)
    }
    await new Promise<void>((resolve) => {
      server?.close(() => resolve())
      setTimeout(resolve, 2000).unref()
    })
  }

  return {
    port: boundPort,
    bind: config.bind,
    serviceInstanceId,
    baseUrl: apiBase,
    capabilities,
    events,
    scans,
    auth,
    config,
    ticketOrigin,
    mintPairingUrl() {
      const ticket = auth.mintTicket(ticketOrigin, hostedPassword !== null)
      if (config.machine) return `${apiBase}/?pair=${ticket}`
      if (config.uiOrigins.length > 0) {
        return `${config.uiOrigins[0]}/?pair=${ticket}&api=${encodeURIComponent(apiBase)}`
      }
      return `${apiBase}/?pair=${ticket}`
    },
    stop,
  }
}

function displayAuthority(config: ResolvedServeConfig, port: number): string {
  const { address } = config.bind
  if (address === '0.0.0.0' || address === '::') return `127.0.0.1:${port}`
  const host = address.includes(':') ? `[${address}]` : address
  return `${host}:${port}`
}
