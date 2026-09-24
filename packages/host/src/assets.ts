import { createHash } from 'node:crypto'
import { readFile, realpath, stat } from 'node:fs/promises'
import { extname, join, normalize, resolve, sep } from 'node:path'

const CSP_TEMPLATE =
  "default-src 'self'; script-src 'self'{HASHES}; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; " +
  "connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors {FRAME_ANCESTORS}"

/** The `frame-ancestors` sources a served document allows. Defaults to nobody. */
export type FrameAncestors = readonly string[]

const CONTENT_TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.webmanifest': 'application/manifest+json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.ico': 'image/x-icon',
  '.txt': 'text/plain; charset=utf-8',
  '.map': 'application/json; charset=utf-8',
  '.xml': 'application/xml; charset=utf-8',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
}

const EXTENSION_PATTERN = /\.[A-Za-z0-9]{1,8}$/

export interface AssetHandler {
  (pathname: string): Promise<Response>
}

function notFound(message: string): Response {
  return new Response(message, { status: 404, headers: { 'content-type': 'text/plain; charset=utf-8' } })
}

/**
 * SvelteKit's client boot script is INLINE in the served document, so a CSP of
 * `script-src 'self'` would block the app from ever mounting. Hash the inline
 * scripts of the actual bytes (cached by mtime+size) and pin them into the CSP
 * — per-document hashes instead of `unsafe-inline`.
 */
const hashCache = new Map<string, string>()

async function documentCspFor(htmlPath: string, html: string, frameAncestors: FrameAncestors): Promise<string> {
  const stats = await stat(htmlPath).catch(() => null)
  // The frame-ancestors sources are part of the cached value, not just the document's bytes:
  // two handlers over the same build may serve different policies (an embedder passes 'self').
  const cacheKey = `${htmlPath}|${stats?.mtimeMs ?? 0}|${stats?.size ?? 0}|${frameAncestors.join(' ')}`
  const cached = hashCache.get(cacheKey)
  if (cached !== undefined) return cached
  const hashes: string[] = []
  const pattern = /<script(?![^>]*\bsrc=)[^>]*>([\s\S]*?)<\/script>/g
  let match: RegExpExecArray | null
  while ((match = pattern.exec(html)) !== null) {
    const digest = createHash('sha256').update(match[1]).digest('base64')
    hashes.push(`'sha256-${digest}'`)
  }
  const header = CSP_TEMPLATE.replace('{HASHES}', hashes.map((hash) => ` ${hash}`).join('')).replace(
    '{FRAME_ANCESTORS}',
    frameAncestors.join(' '),
  )
  if (hashCache.size > 64) hashCache.clear()
  hashCache.set(cacheKey, header)
  return header
}

/**
 * Serves the built SPA. `/api` never reaches this handler (the router claims
 * it first). A path that would escape the web root is a 404 — never the
 * fallback document; asset-shaped paths also 404 instead of falling back, so
 * neither a typo'd asset nor a traversal attempt is ever answered with HTML.
 */
export function createAssetHandler(webRoot: string | null, frameAncestors: FrameAncestors = ["'none'"]): AssetHandler {
  if (webRoot === null) {
    return async () =>
      notFound('no web UI is embedded in this host (start without a web root, or build apps/web first)')
  }
  const root = resolve(webRoot)

  const readInsideRoot = async (
    relative: string,
    isDocument: boolean,
  ): Promise<{ kind: 'outside' } | { kind: 'missing' } | { kind: 'file'; response: Response }> => {
    const absolute = normalize(join(root, relative))
    if (absolute !== root && !absolute.startsWith(`${root}${sep}`)) return { kind: 'outside' }
    let real: string
    try {
      real = await realpath(absolute)
    } catch {
      return { kind: 'missing' }
    }
    const realRoot = await realpath(root)
    if (real !== realRoot && !real.startsWith(`${realRoot}${sep}`)) return { kind: 'outside' }
    let body: Buffer
    try {
      body = await readFile(real)
    } catch {
      return { kind: 'missing' }
    }
    const extension = extname(real).toLowerCase()
    const contentType = CONTENT_TYPES[extension] ?? 'application/octet-stream'
    const headers: Record<string, string> = {
      'content-type': contentType,
      'x-content-type-options': 'nosniff',
    }
    if (isDocument) {
      headers['cache-control'] = 'no-store'
      headers['content-security-policy'] = await documentCspFor(real, body.toString('utf8'), frameAncestors)
      headers['referrer-policy'] = 'no-referrer'
    } else {
      headers['cache-control'] = 'public, max-age=300'
    }
    return { kind: 'file', response: new Response(new Uint8Array(body), { status: 200, headers }) }
  }

  return async (pathname: string) => {
    let decoded: string
    try {
      decoded = decodeURIComponent(pathname)
    } catch {
      return notFound('bad percent-encoding')
    }
    if (decoded.includes('\0')) return notFound('bad path')

    const isDocument = !EXTENSION_PATTERN.test(decoded)
    const direct = await readInsideRoot(decoded, isDocument)
    if (direct.kind === 'file') return direct.response
    if (direct.kind === 'outside') return notFound('bad path')

    if (isDocument) {
      const fallback = await readInsideRoot('/200.html', true)
      if (fallback.kind === 'file') return fallback.response
    }
    return notFound('not found')
  }
}
