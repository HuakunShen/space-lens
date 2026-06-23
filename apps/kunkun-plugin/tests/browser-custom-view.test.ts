/**
 * Browser smoke for the packaged Kunkun custom view.
 * It loads the built Svelte app through Kunkun's headless custom-view host and
 * proves browser JavaScript can discover the host over kkrpc WebSocket.
 */
import { describe, expect, test } from 'bun:test'
import { chromium } from 'playwright'

import { startHeadlessServer } from '@kunkunsh/headless'

describe('Space Lens browser custom-view smoke', () => {
  test('renders the packaged custom view and fails closed on unavailable backend relay', async () => {
    const server = await startHeadlessServer({
      extensions: [new URL('..', import.meta.url).pathname],
      port: 0,
      customViews: [{
        pluginId: 'com.space-lens.app',
        commandName: 'space-lens',
        rootDir: new URL('../dist', import.meta.url).pathname,
      }],
    })
    const browser = await chromium.launch({ headless: true })
    const page = await browser.newPage({ viewport: { width: 1180, height: 760 } })
    const pageErrors: string[] = []
    page.on('pageerror', (error) => pageErrors.push(error.message))

    try {
      const viewUrl = server.getCustomViewUrl({
        pluginId: 'com.space-lens.app',
        commandName: 'space-lens',
        path: 'index.html',
      })

      await page.goto(viewUrl, { waitUntil: 'networkidle', timeout: 10_000 })
      await page.getByRole('heading', { name: 'Space Lens' }).waitFor({ timeout: 5_000 })
      await page.getByRole('button', { name: /Home/ }).waitFor({ timeout: 5_000 })
      await page.getByText('Choose Folder').waitFor({ timeout: 5_000 })

      await page.getByRole('button', { name: /^Scan$/ }).click()
      await page.getByText('Host capability "backend.spawn" is not available in headless runtime').waitFor({
        timeout: 5_000,
      })

      expect(pageErrors).toEqual([])
    } finally {
      await browser.close()
      await server.close()
    }
  }, 20_000)
})
