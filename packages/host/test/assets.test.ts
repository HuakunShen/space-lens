import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'

import { createAssetHandler } from '../src/assets.ts'

describe('asset handler', () => {
  const webRoot = mkdtempSync(join(tmpdir(), 'spacelens-web-'))
  beforeAll(() => {
    writeFileSync(join(webRoot, '200.html'), '<html>fallback</html>')
    writeFileSync(join(webRoot, 'app.js'), 'console.log(1)')
    writeFileSync(join(webRoot, '.secret'), 'nope')
  })
  afterAll(() => {
    rmSync(webRoot, { recursive: true, force: true })
  })

  it('falls back to 200.html for route-shaped paths with document headers', async () => {
    const handler = createAssetHandler(webRoot)
    const response = await handler('/')
    expect(response.status).toBe(200)
    expect(await response.text()).toContain('fallback')
    expect(response.headers.get('cache-control')).toBe('no-store')
    expect(response.headers.get('content-security-policy')).toContain("default-src 'self'")
  })

  it('serves assets with cache headers', async () => {
    const response = await createAssetHandler(webRoot)('/app.js')
    expect(response.status).toBe(200)
    expect(response.headers.get('content-type')).toContain('text/javascript')
    expect(response.headers.get('cache-control')).toContain('max-age=300')
  })

  it('404s missing asset-shaped paths instead of falling back', async () => {
    const response = await createAssetHandler(webRoot)('/missing.js')
    expect(response.status).toBe(404)
  })

  it('refuses traversal and null bytes', async () => {
    const handler = createAssetHandler(webRoot)
    expect((await handler('/../etc/passwd')).status).toBe(404)
    expect((await handler('/%2e%2e/etc/passwd')).status).toBe(404)
    expect((await handler('/%00.html')).status).toBe(404)
  })

  it('404s everything when no web root is embedded', async () => {
    const response = await createAssetHandler(null)('/')
    expect(response.status).toBe(404)
  })

  it("frames nobody by default and 'self' only when an embedder asks for it", async () => {
    const byDefault = await createAssetHandler(webRoot)('/')
    expect(byDefault.headers.get('content-security-policy')).toContain("frame-ancestors 'none'")

    // The DSH panel mounts this host behind its own same-origin route and frames it.
    const embedded = await createAssetHandler(webRoot, ["'self'"])('/')
    expect(embedded.headers.get('content-security-policy')).toContain("frame-ancestors 'self'")
    expect(embedded.headers.get('content-security-policy')).not.toContain("frame-ancestors 'none'")
  })
})
