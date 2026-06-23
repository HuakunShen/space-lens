/**
 * Headless custom-view smoke for the Space Lens Kunkun plugin.
 * It loads the built static view through Kunkun's Electron-free headless host
 * and proves scanner startup fails closed where backend relay is unavailable.
 */
import { afterEach, describe, expect, test } from 'bun:test'
import { homedir } from 'node:os'

import { startHeadlessServer } from '@kunkunsh/headless'
import { createKunkunClient } from '../../web/src/lib/api/kunkun-client'

const originalWindowDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'window')

afterEach(() => {
  if (originalWindowDescriptor) {
    Object.defineProperty(globalThis, 'window', originalWindowDescriptor)
  } else {
    Reflect.deleteProperty(globalThis, 'window')
  }
})

describe('Space Lens headless custom-view smoke', () => {
  test('serves the static plugin and fails closed when backend relay is unavailable', async () => {
    const server = await startHeadlessServer({
      extensions: [new URL('..', import.meta.url).pathname],
      port: 0,
      customViews: [{
        pluginId: 'com.space-lens.app',
        commandName: 'space-lens',
        rootDir: new URL('../dist', import.meta.url).pathname,
      }],
    })

    try {
      const viewUrl = new URL(server.getCustomViewUrl({
        pluginId: 'com.space-lens.app',
        commandName: 'space-lens',
        path: 'index.html',
      }))
      const page = await fetch(viewUrl)
      expect(page.status).toBe(200)
      const html = await page.text()
      expect(html).toContain('./_app/')
      expect(html).not.toContain('"/_app/')

      const assetPath = html.match(/(?:href|src)="(\.\/_app\/[^"]+)"/)?.[1]
      expect(assetPath).toBeDefined()
      const asset = await fetch(new URL(assetPath!, viewUrl))
      expect(asset.status).toBe(200)

      Object.defineProperty(globalThis, 'window', {
        configurable: true,
        value: {
          location: viewUrl,
        },
      })

      const client = createKunkunClient()
      await expect(
        withTimeout(client.startScan({
          paths: [homedir()],
          ignoreHidden: false,
          respectGitignore: true,
          ignoredMode: 'summarize',
          initialDepth: 1,
          maxChildrenPerNode: 10,
        }), 'Space Lens headless startScan'),
      ).rejects.toThrow('Host capability "backend.spawn" is not available in headless runtime')
    } finally {
      await server.close()
    }
  })
})

function withTimeout<T>(promise: Promise<T>, label: string): Promise<T> {
  let timeout: ReturnType<typeof setTimeout> | undefined
  const timeoutPromise = new Promise<never>((_resolve, reject) => {
    timeout = setTimeout(() => reject(new Error(`${label} timed out`)), 5_000)
  })
  return Promise.race([promise, timeoutPromise]).finally(() => {
    if (timeout) clearTimeout(timeout)
  })
}
