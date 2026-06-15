import { serveStatic } from '@hono/node-server/serve-static'
import { serve, upgradeWebSocket } from '@hono/node-server'
import { Hono } from 'hono'
import { cors } from 'hono/cors'
import { createHonoWebSocketHandler } from 'kkrpc/ws/hono'
import { WebSocketServer } from 'ws'
import type { ServerType } from '@hono/node-server'

import { createSpaceLensAPI, errorMessage, statusForError } from './web-service.js'
import type { LoadData } from './web-service.js'
import type {
  CleanupPlanOptions,
  ExecuteCleanupOptions,
  GetChildrenRequest,
  GetNodeRequest,
  SpaceLensAPI,
  StartScanOptions,
} from './web-api.js'

export interface WebServerOptions {
  port?: number
  hostname?: string
  staticDir?: string
  apiOnly?: boolean
}

export interface RunningWebServer {
  server: ServerType
  url: string
}

export function createWebApp(
  options: Pick<WebServerOptions, 'apiOnly' | 'staticDir'> & {
    loadData?: LoadData
  },
): Hono {
  const app = new Hono()
  const api = createSpaceLensAPI({ loadData: options.loadData })
  app.use('/api/*', cors())
  app.get('/rpc', upgradeWebSocket(() => createHonoWebSocketHandler<SpaceLensAPI>({ expose: api })))

  app.post('/api/scans', async (context) => {
    const body = await context.req.json<StartScanOptions>()
    return context.json(await api.startScan(body))
  })

  app.get('/api/scans/:scanId/status', async (context) => {
    try {
      return context.json(await api.getScanStatus(context.req.param('scanId')))
    } catch (error) {
      return context.text(errorMessage(error), 404)
    }
  })

  app.post('/api/scans/:scanId/node', async (context) => {
    const request = await context.req.json<GetNodeRequest>()
    try {
      return context.json(await api.getNode({ ...request, scanId: context.req.param('scanId') || request.scanId }))
    } catch (error) {
      return context.text(errorMessage(error), statusForError(error))
    }
  })

  app.post('/api/scans/:scanId/children', async (context) => {
    const request = await context.req.json<GetChildrenRequest>()
    try {
      return context.json(await api.getChildren({ ...request, scanId: context.req.param('scanId') || request.scanId }))
    } catch (error) {
      return context.text(errorMessage(error), statusForError(error))
    }
  })

  app.post('/api/scans/:scanId/cancel', (context) => {
    void api.cancelScan(context.req.param('scanId'))
    return context.body(null, 204)
  })

  app.post('/api/cleanup/plan', async (context) => {
    const request = await context.req.json<CleanupPlanOptions>()
    return context.json(await api.planCleanup(request))
  })

  app.post('/api/cleanup/execute', async (context) => {
    const request = await context.req.json<ExecuteCleanupOptions>()
    return context.json(await api.executeCleanup(request))
  })

  app.post('/api/host/show-in-file-manager', async (context) => {
    const request = await context.req.json<{ path: string }>()
    try {
      await api.showInFileManager?.(request.path)
      return context.body(null, 204)
    } catch (error) {
      return context.text(errorMessage(error), statusForError(error))
    }
  })
  app.post('/api/host/open-in-terminal', async (context) => {
    const request = await context.req.json<{ path: string }>()
    try {
      await api.openInTerminal?.(request.path)
      return context.body(null, 204)
    } catch (error) {
      return context.text(errorMessage(error), statusForError(error))
    }
  })

  if (!options.apiOnly && options.staticDir) {
    app.use('/assets/*', serveStatic({ root: options.staticDir }))
    app.use('/favicon.*', serveStatic({ root: options.staticDir }))
    app.use('*', serveStatic({ root: options.staticDir }))
    app.get('*', serveStatic({ root: options.staticDir, path: 'index.html' }))
  }

  return app
}

export function startWebServer(options: WebServerOptions): RunningWebServer {
  const app = createWebApp({ apiOnly: options.apiOnly, staticDir: options.staticDir })
  const port = options.port ?? 8757
  const hostname = options.hostname ?? '127.0.0.1'
  const webSocketServer = new WebSocketServer({ noServer: true })
  const server = serve({ fetch: app.fetch, port, hostname, websocket: { server: webSocketServer } })
  server.on('close', () => webSocketServer.close())
  return {
    server,
    url: `http://${hostname}:${port}`,
  }
}
