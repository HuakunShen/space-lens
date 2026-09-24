/**
 * Host half of the Space Lens plugin for the DeepSeek Harness.
 *
 * It adds one thing to the Harness host process: the Space Lens workbench for the
 * directory the current session is working in, rendered by Space Lens's own SPA
 * inside the Harness Web UI.
 *
 * The shape follows the Refyard plugin's, with two product differences:
 *
 * - **A Space Lens server per approved root.** `startServe` fixes the scan roots at
 *   startup and `ScanManager.containRoot` refuses anything outside them — that
 *   containment is the product's security model, so the plugin does not reach into
 *   it. Instead it starts one loopback, port-0 server per directory a session
 *   actually asks about, and proxies that server's route. "Which folder is open"
 *   becomes "which server answers this mount", one per project.
 * - **Read-only by default.** The server is started without cleanup scopes, so the
 *   embedded workbench can look but never trash: deleting from a panel a session
 *   opened implicitly should never be the easy path.
 *
 * Otherwise: no second origin (the Harness route proxies a port-0 listener), the
 * pairing ticket still exists (minted here in trusted code, spent by the frame),
 * and directories are approved on demand — one server per session `cwd`, never a
 * parent, never because the plugin started.
 */
import { request as httpRequest, type IncomingMessage, type ServerResponse } from 'node:http'
import { realpath } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { startServe, type RunningServer } from '@space-lens/host'

/** Where the workbench is mounted on the Harness web server. The client half reads it too. */
const PREFIX = '/space-lens'
/** The route the panel asks for its frame URL. */
const CONTEXT_ROUTE = `${PREFIX}/dsh/context`

/** Wait for the web server before applying, so the route registers whenever it activates. */
export const inject = ['webServer']

/** The part of a Session this plugin reads: its identity and the directory it works in. */
interface SessionAnnouncement {
  readonly id?: unknown
  readonly header?: {
    readonly cwd?: unknown
    readonly parentSession?: unknown
    readonly origin?: unknown
  }
}

/** The part of the Harness web server this plugin needs. */
interface WebServerLike {
  readonly port?: number
  register(route: {
    kind: 'exact' | 'prefix'
    path: string
    handler: (req: IncomingMessage, res: ServerResponse) => void | Promise<void>
  }): () => void
}

/** The slice of the Harness host context this plugin uses (see the Refyard plugin's note). */
interface HostContext {
  readonly logger: {
    info(message: string): void
    warn(message: string): void
    error(message: string): void
  }
  get(name: 'webServer'): WebServerLike | undefined
  on(name: 'session/created', listener: (session: SessionAnnouncement) => void): unknown
  on(name: 'session/event', listener: (session: SessionAnnouncement, event: unknown) => void): unknown
  effect(install: () => void | (() => void), label?: string): unknown
}

export function apply(ctx: HostContext): void {
  const webServer = ctx.get('webServer')
  if (webServer === undefined) {
    ctx.logger.warn('space-lens: no webServer service in this composition; the scan panel has nothing to mount on')
    return
  }

  // `dist/host.js` sits beside the SPA copy the build staged for the embedder.
  const webRoot = join(dirname(fileURLToPath(import.meta.url)), 'web')

  /** Every Session this process has seen, by id, newest observation last. */
  const candidates = new Map<string, string>()

  function observe(session: SessionAnnouncement | undefined): void {
    const id = typeof session?.id === 'string' ? session.id : null
    const cwd = session?.header?.cwd
    if (id === null || typeof cwd !== 'string' || cwd === '') {
      return
    }
    // Re-inserting keeps Map iteration order equal to recency, which is what makes "the
    // Session the user is most likely looking at" answerable without asking the browser.
    candidates.delete(id)
    candidates.set(id, cwd)
  }

  ctx.on('session/created', (session) => observe(session))
  // A Session that already existed when this plugin activated never announces itself
  // again; any durable event re-teaches the host where that Session works.
  ctx.on('session/event', (session) => observe(session))

  /** One running server per canonical directory, created on demand, never restarted. */
  const servers = new Map<string, Promise<RunningServer> | RunningServer>()
  /** The origins this plugin serves and mints pairing tickets for: loopback, this port. */
  let allowedOrigins: readonly string[] = []

  ctx.effect(
    () => () => {
      for (const entry of servers.values()) {
        const running = entry instanceof Promise ? null : entry
        if (running !== null) void running.stop()
      }
      servers.clear()
    },
    'space-lens: server lifecycle',
  )

  function portOf(): number | null {
    const port = webServer?.port
    return typeof port === 'number' && Number.isInteger(port) && port > 0 ? port : null
  }

  function originsFor(port: number): readonly string[] {
    // Bracketed IPv6 is a valid origin but not one `normalizeOrigin` accepts,
    // and the frame is reached over `127.0.0.1` anyway.
    return [`http://127.0.0.1:${port}`, `http://localhost:${port}`]
  }

  /**
   * The server for one directory: a full Space Lens host on a private loopback
   * port, rooted at exactly that directory, read-only, framed only by the page
   * that proxies it. A failed start must be retryable — caching it would keep
   * the panel dead for the life of the process.
   */
  async function ensureServer(rawPath: string): Promise<RunningServer> {
    const port = portOf()
    if (port === null) {
      throw new Error('the Harness web server has no listening port yet')
    }
    allowedOrigins = originsFor(port)
    const canonical = await realpath(rawPath)
    const existing = servers.get(canonical)
    if (existing !== undefined && !(existing instanceof Promise)) {
      return existing
    }
    const starting =
      existing ??
      startServe({
        host: '127.0.0.1',
        port: 0,
        // Exactly this directory — the containment the product already enforces.
        roots: [canonical],
        webRoot,
        // The frame is same-origin with the Harness page; the service is told
        // which origin may call it, and tickets bind to that origin.
        uiOrigins: allowedOrigins,
        frameAncestors: ["'self'"],
        allowCleanup: false,
        open: false,
        json: false,
        ticketTtlSeconds: 120,
      })
    servers.set(canonical, starting)
    try {
      const running = await starting
      servers.set(canonical, running)
      ctx.logger.info(
        `space-lens: workbench for ${canonical} listening on http://127.0.0.1:${running.port} behind ${PREFIX}`,
      )
      return running
    } catch (error) {
      servers.delete(canonical)
      throw error
    }
  }

  /** The most recently observed Session's directory — the one a person is working in. */
  function bestKnownDirectory(): string | null {
    let newest: string | null = null
    for (const directory of candidates.values()) {
      newest = directory
    }
    return newest
  }

  /**
   * The API base is directory-scoped: `/space-lens/d/<encoded dir>`.
   *
   * The SPA composes every API call as `${base}/api/v1/...` from the `api`
   * parameter alone — its own fetches carry no query — so the directory a frame
   * is about has to travel *inside* the base or two Sessions' panels could not
   * be told apart. The segment is this plugin's own; the upstream never sees it.
   */
  function directorySegment(directory: string): string {
    return `${PREFIX}/d/${encodeURIComponent(directory)}`
  }

  /**
   * Split a request path into its directory and the upstream remainder.
   *
   * The `d/<dir>` segment wins: it is what the frame's own API calls carry. A
   * document request instead names the directory in its query, because the
   * redirect to it is composed before any API base exists.
   */
  function splitDirectory(url: URL): { directory: string | null; upstream: string } {
    const rest = url.pathname.slice(PREFIX.length)
    const segment = rest.match(/^\/d\/([^/]+)(\/.*)?$/)
    if (segment !== null) {
      return {
        directory: decodeURIComponent(segment[1] ?? ''),
        upstream: `${segment[2] ?? '/'}${url.search}`,
      }
    }
    return { directory: requestedDirectory(url), upstream: `${rest}${url.search}` }
  }

  /** The directory this request is about: an explicit one, a named Session's, or the newest. */
  function requestedDirectory(url: URL): string | null {
    const named = url.searchParams.get('dir')
    if (named !== null && named.trim() !== '') {
      return named.trim()
    }
    const sessionId = url.searchParams.get('session')
    if (sessionId !== null) {
      const exact = candidates.get(sessionId)
      if (exact !== undefined) {
        return exact
      }
    }
    return bestKnownDirectory()
  }

  function sendJson(res: ServerResponse, status: number, body: unknown): void {
    res.writeHead(status, {
      'content-type': 'application/json; charset=utf-8',
      'cache-control': 'no-store',
    })
    res.end(JSON.stringify(body))
  }

  function sendProblem(res: ServerResponse, status: number, message: string): void {
    sendJson(res, status, { problem: { status, message } })
  }

  /** The panel's frame URL, and the directory it will show. */
  async function contextPayload(url: URL): Promise<{
    readonly rootPath: string | null
    readonly panelUrl: string
  }> {
    const directory = requestedDirectory(url)
    const query = new URLSearchParams()
    if (directory !== null) {
      query.set('dir', directory)
    }
    const suffix = query.size === 0 ? '' : `?${query.toString()}`
    return { rootPath: directory, panelUrl: `${PREFIX}/${suffix}` }
  }

  /**
   * Answer a document request with a pairing redirect plus the two things the
   * SPA needs to go from "loaded" to "scanning": an `api` base naming this
   * mount, and `autoscan`, which starts the root scan once pairing lands.
   */
  async function serveDocument(res: ServerResponse, url: URL, origin: string): Promise<void> {
    const directory = requestedDirectory(url)
    if (directory === null) {
      sendProblem(res, 404, 'no session directory is known to the Space Lens panel yet')
      return
    }
    let running: RunningServer
    let canonical: string
    try {
      running = await ensureServer(directory)
      canonical = await realpath(directory)
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      sendProblem(res, 500, `the Space Lens workbench did not start: ${message}`)
      return
    }
    // The pairing URL names whichever origin the config settled on; only the
    // ticket parameter is needed — the frame's document lives on this mount.
    const ticket = new URL(running.mintPairingUrl()).searchParams.get('pair')
    if (ticket === null) {
      sendProblem(res, 500, 'the service did not issue a pairing ticket')
      return
    }
    url.searchParams.set('api', `${origin}${directorySegment(canonical)}`)
    url.searchParams.set('pair', ticket)
    url.searchParams.set('autoscan', '1')
    res.writeHead(302, {
      location: `${url.pathname}${url.search}`,
      'cache-control': 'no-store',
      'referrer-policy': 'no-referrer',
    })
    res.end()
  }

  /** Forward one request to the loopback Space Lens server and stream the answer back. */
  function proxy(req: IncomingMessage, res: ServerResponse, upstreamPort: number, path: string): void {
    const upstream = httpRequest({
      host: '127.0.0.1',
      port: upstreamPort,
      path,
      method: req.method,
      headers: {
        ...req.headers,
        // The service answers only on its own authority, and this is that authority.
        host: `127.0.0.1:${upstreamPort}`,
      },
    })
    upstream.on('response', (answer) => {
      res.writeHead(answer.statusCode ?? 502, answer.headers)
      answer.pipe(res)
    })
    upstream.on('error', (error: Error) => {
      ctx.logger.error(`space-lens: upstream request failed: ${error.message}`)
      if (!res.headersSent) {
        sendProblem(res, 502, 'the Space Lens workbench did not answer')
      } else {
        res.end()
      }
    })
    req.on('aborted', () => upstream.destroy())
    req.pipe(upstream)
  }

  async function handle(req: IncomingMessage, res: ServerResponse): Promise<void> {
    const port = portOf()
    if (port === null) {
      sendProblem(res, 503, 'the Harness web server has no listening port yet')
      return
    }
    const origins = originsFor(port)
    // The same three questions the product asks behind the proxy: Host, Origin,
    // Sec-Fetch-Site. A cross-site page cannot use this route to reach a scan.
    const host = req.headers.host
    const origin = req.headers.origin
    if (typeof host !== 'string' || host === '' || !origins.includes(`http://${host}`)) {
      sendProblem(res, 403, 'this panel is served on a loopback origin only')
      return
    }
    if (typeof origin === 'string' && origin !== '' && !origins.includes(origin)) {
      sendProblem(res, 403, 'request origin is not allowed')
      return
    }

    const url = new URL(req.url ?? '/', `http://${req.headers.host ?? '127.0.0.1'}`)
    // The mount point itself is a directory: without the slash, the frame's own
    // relative resolution would step out of the mount on the next navigation.
    if (url.pathname === PREFIX) {
      res.writeHead(308, {
        location: `${PREFIX}/${url.search}`,
        'cache-control': 'no-store',
      })
      res.end()
      return
    }
    if (url.pathname === CONTEXT_ROUTE) {
      sendJson(res, 200, await contextPayload(url))
      return
    }

    const isDocument =
      (req.method === 'GET' || req.method === 'HEAD') && (req.headers.accept ?? '').includes('text/html')
    if (isDocument && !url.searchParams.has('pair')) {
      await serveDocument(res, url, `http://${host}`)
      return
    }

    // Past the document: API calls and assets carry the directory inside the
    // API base (`d/<dir>`); documents and bare assets fall back to the query,
    // then to the newest known directory.
    const split = splitDirectory(url)
    const directory = split.directory ?? bestKnownDirectory()
    if (directory === null) {
      sendProblem(res, 404, 'no directory is known to the Space Lens panel yet')
      return
    }
    const running = await ensureServer(directory)
    proxy(req, res, running.port, split.upstream === '' ? '/' : split.upstream)
  }

  ctx.effect(
    () =>
      webServer.register({
        kind: 'prefix',
        path: PREFIX,
        handler(req, res) {
          void handle(req, res).catch((error: unknown) => {
            const message = error instanceof Error ? error.message : String(error)
            ctx.logger.error(`space-lens: panel request failed: ${message}`)
            if (!res.headersSent) {
              sendProblem(res, 500, `the Space Lens panel failed: ${message}`)
            } else {
              res.end()
            }
          })
        },
      }),
    'space-lens: web route',
  )
}
