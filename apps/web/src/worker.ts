/**
 * Asset-only Worker. It never runs the engine, never sees a token, and never
 * proxies the API: the browser talks to the `spacelens serve` host directly
 * (cross-origin, exactly-allowlisted). What this Worker does is security
 * headers, an honest JSON 404 for /api typos, and the fail-closed
 * `connect-src` allowlist for the deployed UI.
 */
interface Env {
  /** Static Assets binding (wrangler.jsonc). */
  ASSETS: { fetch(request: Request): Promise<Response> }
  /** Comma-separated exact https origins the deployed UI may call. Empty = same-origin only. */
  PUBLIC_API_ORIGINS?: string
}

const SECURITY_HEADERS: Record<string, string> = {
  'x-content-type-options': 'nosniff',
  'referrer-policy': 'no-referrer',
  'x-frame-options': 'DENY',
  'permissions-policy': 'camera=(), microphone=(), geolocation=()',
  'cross-origin-opener-policy': 'same-origin',
  'cross-origin-resource-policy': 'same-origin',
  'strict-transport-security': 'max-age=31536000; includeSubDomains',
}

function connectSrc(env: Env): string {
  const extra = (env.PUBLIC_API_ORIGINS ?? '')
    .split(',')
    .map((origin) => origin.trim())
    .filter((origin) => /^https:\/\/[a-z0-9.-]+(:\d+)?$/i.test(origin))
  return ["'self'", ...extra].join(' ')
}

function documentCsp(env: Env): string {
  return [
    "default-src 'self'",
    "script-src 'self'",
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data:",
    "font-src 'self'",
    `connect-src ${connectSrc(env)}`,
    "object-src 'none'",
    "base-uri 'none'",
    "form-action 'none'",
    "frame-ancestors 'none'",
  ].join('; ')
}

function withHeaders(response: Response, env: Env): Response {
  const next = new Response(response.body, response)
  for (const [name, value] of Object.entries(SECURITY_HEADERS)) next.headers.set(name, value)
  const contentType = next.headers.get('content-type') ?? ''
  if (contentType.includes('text/html') || contentType.includes('manifest')) {
    next.headers.set('content-security-policy', documentCsp(env))
    next.headers.set('cache-control', 'no-store')
  } else if (response.url.includes('/_app/immutable/')) {
    next.headers.set('cache-control', 'public, max-age=31536000, immutable')
  } else {
    next.headers.set('cache-control', 'public, max-age=300')
  }
  return next
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url)
    if (request.method !== 'GET' && request.method !== 'HEAD') {
      return Response.json(
        { problem: { code: 'InvalidRequest', message: 'this deployment serves static assets only', retryable: false } },
        { status: 405 },
      )
    }
    if (url.pathname === '/api' || url.pathname.startsWith('/api/')) {
      // an API typo must be answered with JSON, never with the SPA shell
      return Response.json(
        {
          problem: {
            code: 'NotFound',
            message: 'this deployment hosts no API; point the UI at your spacelens serve host',
            retryable: false,
          },
        },
        { status: 404, headers: SECURITY_HEADERS },
      )
    }
    const response = await env.ASSETS.fetch(request)
    return withHeaders(response, env)
  },
}
