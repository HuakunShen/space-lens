import { serveStatic } from '@hono/node-server/serve-static'
import { serve, upgradeWebSocket } from '@hono/node-server'
import { Hono } from 'hono'
import { createHonoWebSocketHandler } from 'kkrpc/ws/hono'
import { WebSocketServer } from 'ws'
import type { ServerType } from '@hono/node-server'

import { createSpaceLensAPI } from './web-service.js'
import type { LoadData } from './web-service.js'
import type { SpaceLensAPI } from './web-api.js'
import {
  assertCapability,
  authorizeBootToken,
  authorizeRpcUpgrade,
  bootUrlFor,
  createWebSecurityContext,
  rpcUrlFor,
  type WebSecurityContext,
} from './web-security.js'

export interface WebServerOptions {
  port?: number
  hostname?: string
  staticDir?: string
  apiOnly?: boolean
  loadData?: LoadData
  allowCleanupExecute?: boolean
}

export interface RunningWebServer {
  server: ServerType
  url: string
  appUrl: string
  rpcUrl: string
  bootUrl: string
  security: WebSecurityContext
}

export function createWebApp(
  options: Pick<WebServerOptions, 'apiOnly' | 'staticDir'> & {
    loadData?: LoadData
    security?: WebSecurityContext
  },
): Hono {
  const app = new Hono()
  const api = createGuardedSpaceLensAPI(createSpaceLensAPI({ loadData: options.loadData }), options.security)

  if (options.security) {
    app.get('/space-lens.boot.json', (context) => {
      if (!authorizeBootToken(options.security!, context.req.query('token'))) {
        return context.json({ error: 'Unauthorized' }, 401)
      }
      return context.json({
        mode: 'rpc',
        wsRpcUrl: rpcUrlFor(options.security!),
      })
    })
  }

  app.get('/rpc', upgradeWebSocket((context) => {
    if (options.security) {
      const authError = authorizeRpcUpgrade(options.security, {
        token: context.req.query('token'),
        host: context.req.header('host'),
        origin: context.req.header('origin'),
      })
      if (authError) {
        return {
          onOpen(_event, ws) {
            ws.close(1008, authError.message)
          },
        }
      }
    }
    return createHonoWebSocketHandler<SpaceLensAPI>({ expose: api })
  }))

  if (!options.apiOnly && options.staticDir) {
    app.use('/assets/*', serveStatic({ root: options.staticDir }))
    app.use('/favicon.*', serveStatic({ root: options.staticDir }))
    app.use('*', serveStatic({ root: options.staticDir }))
    app.get('*', serveStatic({ root: options.staticDir, path: 'index.html' }))
  }

  return app
}

export function startWebServer(options: WebServerOptions): RunningWebServer {
  const port = options.port ?? 8757
  const hostname = options.hostname ?? '127.0.0.1'
  const security = createWebSecurityContext({
    origin: `http://${hostname}:${port}`,
    allowCleanupExecute: options.allowCleanupExecute,
  })
  const app = createWebApp({
    apiOnly: options.apiOnly,
    staticDir: options.staticDir,
    loadData: options.loadData,
    security,
  })
  const webSocketServer = new WebSocketServer({ noServer: true })
  const server = serve({ fetch: app.fetch, port, hostname, websocket: { server: webSocketServer } })
  server.on('listening', () => {
    security.origin = resolveServerUrl(server, hostname, port)
  })
  server.on('close', () => webSocketServer.close())
  return {
    server,
    get url() {
      return resolveServerUrl(server, hostname, port)
    },
    get appUrl() {
      return appUrlFor(resolveServerUrl(server, hostname, port), rpcUrlFor(security))
    },
    get rpcUrl() {
      return rpcUrlFor(security)
    },
    get bootUrl() {
      return bootUrlFor(security)
    },
    security,
  }
}

function appUrlFor(baseUrl: string, rpcUrl: string): string {
  const url = new URL(baseUrl)
  url.pathname = '/'
  url.search = ''
  url.searchParams.set('spaceLensRpc', rpcUrl)
  url.hash = ''
  return url.toString()
}

function createGuardedSpaceLensAPI(api: SpaceLensAPI, security: WebSecurityContext | undefined): SpaceLensAPI {
  const requireCapability = security
    ? (capability: Parameters<typeof assertCapability>[1]) => assertCapability(security, capability)
    : () => undefined

  return {
    async getScanTargets() {
      requireCapability('scan:read')
      return api.getScanTargets()
    },
    async startScan(options) {
      requireCapability('scan:start')
      return api.startScan(options)
    },
    async getNode(request) {
      requireCapability('scan:read')
      return api.getNode(request)
    },
    async getChildren(request) {
      requireCapability('scan:read')
      return api.getChildren(request)
    },
    async getScanStatus(scanId) {
      requireCapability('scan:read')
      return api.getScanStatus(scanId)
    },
    async cancelScan(scanId) {
      requireCapability('scan:cancel')
      return api.cancelScan(scanId)
    },
    async planCleanup(options) {
      requireCapability('cleanup:plan')
      return api.planCleanup(options)
    },
    async executeCleanup(options) {
      requireCapability('cleanup:execute')
      return api.executeCleanup(options)
    },
    async showInFileManager(path) {
      requireCapability('host:reveal')
      return api.showInFileManager?.(path)
    },
    async openInTerminal(path) {
      requireCapability('host:reveal')
      return api.openInTerminal?.(path)
    },
  }
}

function resolveServerUrl(server: ServerType, hostname: string, fallbackPort: number): string {
  const address = server.address()
  const port = typeof address === 'object' && address ? address.port : fallbackPort
  return `http://${hostname}:${port}`
}
