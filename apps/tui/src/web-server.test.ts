import assert from 'node:assert/strict'
import { once } from 'node:events'
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import type { AddressInfo } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'

import { RPCChannel } from 'kkrpc'
import { webSocketClientTransport } from 'kkrpc/ws'
import WebSocket from 'ws'

import { createWebApp, startWebServer } from './web-server.js'
import { createWebSecurityContext } from './web-security.js'
import type { SpaceLensData } from './scanner.js'

interface ScanResponse {
  scanId: string
  rootIds: string[]
}

interface ScanStatusResponse {
  scanId: string
  state: 'idle' | 'scanning' | 'ready' | 'cancelled' | 'failed'
  message: string
  progress: number | null
  currentPath: string | null
  bytesScanned: number
  entriesScanned: number
  rootIds: string[]
  label: string | null
  updatedAt: string
}

interface TreeSliceResponse {
  children: unknown[]
  omittedCount: number
  tree: {
    children: unknown[]
  }
}

interface ChildrenPageResponse {
  items: unknown[]
  limit: number
  offset: number
  total: number
}

interface CleanupOutcomeResponse {
  removed: Array<{ path: string }>
  bytesRemoved: number
  errors: string[]
}

interface ScanTargetResponse {
  id: string
  label: string
  path: string
  kind: 'volume' | 'folder' | 'multi-folder'
  description: string
  size: number
  used?: number
}

interface RpcAPI {
  getScanTargets(): Promise<ScanTargetResponse[]>
  startScan(options: {
    paths: string[]
    ignoreHidden: boolean
    respectGitignore: boolean
    ignoredMode: 'summarize' | 'exclude'
    initialDepth: number
    maxChildrenPerNode: number
  }): Promise<ScanResponse>
  getScanStatus(scanId: string): Promise<ScanStatusResponse>
  getNode(request: {
    scanId: string
    nodeId: string
    depth: number
    maxChildrenPerNode: number
  }): Promise<TreeSliceResponse>
  getChildren(request: {
    scanId: string
    nodeId: string
    offset: number
    limit: number
    sort: 'size' | 'name'
  }): Promise<ChildrenPageResponse>
  executeCleanup(options: {
    scanId: string
    entries: Array<{
      id: string
      scanId: string
      nodeId: string
      path: string
      name: string
      size: number
      addedAt: string
    }>
  }): Promise<CleanupOutcomeResponse>
}

test('web server exposes scan APIs through kkrpc websocket', async () => {
  const root = mkdtempSync(join(tmpdir(), 'space-lens-web-rpc-'))

  try {
    writeFileSync(join(root, 'alpha.txt'), 'alpha')
    const running = startWebServer({
      apiOnly: true,
      staticDir: root,
      port: 0,
      hostname: '127.0.0.1',
    })
    const { server } = running
    await once(server, 'listening')
    const address = server.address() as AddressInfo
    assert.equal(running.url, `http://127.0.0.1:${address.port}`)
    const appUrl = new URL(running.appUrl)
    assert.equal(appUrl.origin, running.url)
    assert.equal(appUrl.searchParams.get('spaceLensRpc'), running.rpcUrl)
    const channel = new RPCChannel<object, RpcAPI>(
      webSocketClientTransport({ url: running.rpcUrl }),
      { timeout: 500 },
    )
    const api = channel.getAPI()

    try {
      const targets = await withTimeout(api.getScanTargets())
      assert.ok(targets.length > 0)
      assert.ok(targets.every((target) => target.path.length > 0))

      const scan = await withTimeout(
        api.startScan({
          paths: [root],
          ignoreHidden: false,
          respectGitignore: false,
          ignoredMode: 'summarize',
          initialDepth: 1,
          maxChildrenPerNode: 10,
        }),
      )
      const ready = await waitForRpcReady(api, scan.scanId)
      const slice = await api.getNode({
        scanId: scan.scanId,
        nodeId: ready.rootIds[0],
        depth: 1,
        maxChildrenPerNode: 10,
      })

      assert.equal(ready.state, 'ready')
      assert.equal(slice.children.length, 1)
      assert.equal(slice.tree.children.length, 1)
    } finally {
      channel.destroy()
      server.close()
    }
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('web server serves boot config only with boot token', async () => {
  const root = mkdtempSync(join(tmpdir(), 'space-lens-web-auth-'))

  try {
    const app = createWebApp({
      staticDir: root,
      security: createWebSecurityContext({
        origin: 'http://127.0.0.1:8757',
        rpcToken: 'rpc-test-token',
        bootToken: 'boot-test-token',
      }),
    })

    const rejected = await app.request('/space-lens.boot.json')
    assert.equal(rejected.status, 401)

    const accepted = await app.request('/space-lens.boot.json?token=boot-test-token')
    assert.equal(accepted.status, 200)
    assert.deepEqual(await accepted.json(), {
      mode: 'rpc',
      wsRpcUrl: 'ws://127.0.0.1:8757/rpc?token=rpc-test-token',
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('web server does not expose legacy REST scan routes', async () => {
  const root = mkdtempSync(join(tmpdir(), 'space-lens-web-no-rest-'))

  try {
    const app = createWebApp({ staticDir: root })
    const response = await app.request('/api/scans', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: '{}',
    })

    assert.equal(response.status, 404)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('web server rejects websocket RPC requests without token', async () => {
  const root = mkdtempSync(join(tmpdir(), 'space-lens-web-rpc-auth-'))

  try {
    const running = startWebServer({
      apiOnly: true,
      staticDir: root,
      port: 0,
      hostname: '127.0.0.1',
    })
    const { server } = running
    await once(server, 'listening')
    const address = server.address() as AddressInfo
    const socket = new WebSocket(`ws://127.0.0.1:${address.port}/rpc`, {
      origin: running.url,
    })

    try {
      const close = await once(socket, 'close')
      assert.equal(close[0], 1008)
    } finally {
      socket.close()
      server.close()
    }
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('web server rejects websocket RPC requests with wrong host or origin', async () => {
  const root = mkdtempSync(join(tmpdir(), 'space-lens-web-rpc-origin-'))

  try {
    const running = startWebServer({
      apiOnly: true,
      staticDir: root,
      port: 0,
      hostname: '127.0.0.1',
    })
    const { server } = running
    await once(server, 'listening')
    const address = server.address() as AddressInfo

    const wrongOrigin = new WebSocket(running.rpcUrl, {
      origin: 'http://evil.example',
    })
    const wrongOriginClose = await once(wrongOrigin, 'close')
    assert.equal(wrongOriginClose[0], 1008)

    const wrongHost = new WebSocket(`ws://127.0.0.1:${address.port}/rpc?token=${running.security.rpcToken}`, {
      headers: {
        host: 'evil.example',
      },
      origin: running.url,
    })
    const wrongHostClose = await once(wrongHost, 'close')
    assert.equal(wrongHostClose[0], 1008)

    server.close()
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('web server denies cleanup execution unless explicitly enabled', async () => {
  const root = mkdtempSync(join(tmpdir(), 'space-lens-web-cleanup-denied-'))

  try {
    const target = join(root, 'keep-me.txt')
    writeFileSync(target, 'keep me')
    const running = startWebServer({
      apiOnly: true,
      staticDir: root,
      port: 0,
      hostname: '127.0.0.1',
    })
    await once(running.server, 'listening')
    const channel = new RPCChannel<object, RpcAPI>(
      webSocketClientTransport({ url: running.rpcUrl }),
      { timeout: 500 },
    )
    const api = channel.getAPI()

    try {
      await assert.rejects(
        api.executeCleanup({
          scanId: 'scan-test',
          entries: [
            {
              id: 'scan-test:0',
              scanId: 'scan-test',
              nodeId: '0',
              path: target,
              name: 'keep-me.txt',
              size: 7,
              addedAt: new Date().toISOString(),
            },
          ],
        }),
        /Capability denied: cleanup:execute/,
      )
    } finally {
      channel.destroy()
      running.server.close()
    }
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('web server deletes manually collected paths', async () => {
  const root = mkdtempSync(join(tmpdir(), 'space-lens-web-manual-delete-'))

  try {
    const target = join(root, 'delete-me.txt')
    writeFileSync(target, 'delete me')
    const running = startWebServer({
      apiOnly: true,
      staticDir: root,
      port: 0,
      hostname: '127.0.0.1',
      allowCleanupExecute: true,
    })
    await once(running.server, 'listening')
    const channel = new RPCChannel<object, RpcAPI>(
      webSocketClientTransport({ url: running.rpcUrl }),
      { timeout: 500 },
    )
    const api = channel.getAPI()

    try {
      const outcome = await api.executeCleanup({
        scanId: 'scan-test',
        entries: [
          {
            id: 'scan-test:0',
            scanId: 'scan-test',
            nodeId: '0',
            path: target,
            name: 'delete-me.txt',
            size: 9,
            addedAt: new Date().toISOString(),
          },
        ],
      })

      assert.deepEqual(outcome.errors, [])
      assert.equal(outcome.removed.length, 1)
      assert.equal(outcome.removed[0].path, target)
      assert.equal(outcome.bytesRemoved, 9)
    } finally {
      channel.destroy()
      running.server.close()
    }
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('web server starts scans in the background and reports progress status', async () => {
  const root = mkdtempSync(join(tmpdir(), 'space-lens-web-progress-'))

  try {
    writeFileSync(join(root, 'alpha.txt'), 'alpha')
    let finishScan!: (data: SpaceLensData) => void
    const running = startWebServer({
      apiOnly: true,
      staticDir: root,
      port: 0,
      hostname: '127.0.0.1',
      loadData: async (_options, onProgress) => {
        onProgress({
          path: join(root, 'alpha.txt'),
          bytesScanned: 128,
          entriesScanned: 1,
        })
        return new Promise<SpaceLensData>((resolve) => {
          finishScan = resolve
        })
      },
    })
    await once(running.server, 'listening')
    const channel = new RPCChannel<object, RpcAPI>(
      webSocketClientTransport({ url: running.rpcUrl }),
      { timeout: 500 },
    )
    const api = channel.getAPI()

    try {
      const scan = await api.startScan({
        paths: [root],
        ignoreHidden: false,
        respectGitignore: false,
        ignoredMode: 'summarize',
        initialDepth: 1,
        maxChildrenPerNode: 10,
      })

      const scanning = await waitForRpcProgress(api, scan.scanId)
      assert.equal(scanning.state, 'scanning')
      assert.equal(scanning.currentPath, join(root, 'alpha.txt'))
      assert.equal(scanning.bytesScanned, 128)
      assert.equal(scanning.entriesScanned, 1)

      finishScan({
        scanTrees: [
          {
            name: 'alpha.txt',
            path: join(root, 'alpha.txt'),
            size: 128,
            children: [],
            depth: 0,
            ignored: false,
            collapsed: false,
          },
        ],
        plan: {
          entries: [],
          totalSize: 0,
          errors: [],
        },
      })

      await waitForRpcReady(api, scan.scanId)
    } finally {
      channel.destroy()
      running.server.close()
    }
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('web server returns startScan before synchronous scanner work begins', async () => {
  const root = mkdtempSync(join(tmpdir(), 'space-lens-web-sync-scan-'))

  try {
    let scanStarted = false
    const running = startWebServer({
      apiOnly: true,
      staticDir: root,
      port: 0,
      hostname: '127.0.0.1',
      loadData: () => {
        scanStarted = true
        return {
          scanTrees: [
            {
              name: 'sync-root',
              path: root,
              size: 1,
              children: [],
              depth: 0,
              ignored: false,
              collapsed: false,
            },
          ],
          plan: {
            entries: [],
            totalSize: 0,
            errors: [],
          },
        }
      },
    })
    await once(running.server, 'listening')
    const channel = new RPCChannel<object, RpcAPI>(
      webSocketClientTransport({ url: running.rpcUrl }),
      { timeout: 500 },
    )
    const api = channel.getAPI()

    try {
      const scan = await api.startScan({
        paths: [root],
        ignoreHidden: false,
        respectGitignore: false,
        ignoredMode: 'summarize',
        initialDepth: 1,
        maxChildrenPerNode: 10,
      })

      assert.equal(scanStarted, false)
      const initialStatus = await api.getScanStatus(scan.scanId)
      assert.equal(initialStatus.state, 'scanning')

      await waitForRpcReady(api, scan.scanId)
      assert.equal(scanStarted, true)
    } finally {
      channel.destroy()
      running.server.close()
    }
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('web server returns bounded tree slices and paged child lists', async () => {
  const root = mkdtempSync(join(tmpdir(), 'space-lens-web-server-'))

  try {
    for (let index = 0; index < 130; index += 1) {
      const child = join(root, `child-${String(index).padStart(3, '0')}`)
      mkdirSync(child)
      writeFileSync(join(child, 'payload.txt'), 'x'.repeat(index + 1))
    }

    const running = startWebServer({
      apiOnly: true,
      staticDir: root,
      port: 0,
      hostname: '127.0.0.1',
    })
    await once(running.server, 'listening')
    const channel = new RPCChannel<object, RpcAPI>(
      webSocketClientTransport({ url: running.rpcUrl }),
      { timeout: 500 },
    )
    const api = channel.getAPI()

    try {
      const scan = await api.startScan({
        paths: [root],
        ignoreHidden: false,
        respectGitignore: false,
        ignoredMode: 'summarize',
        initialDepth: 1,
        maxChildrenPerNode: 10,
      })

      const ready = await waitForRpcReady(api, scan.scanId)
      const slice = await api.getNode({
        scanId: scan.scanId,
        nodeId: ready.rootIds[0],
        depth: 1,
        maxChildrenPerNode: 10,
      })

      assert.equal(slice.children.length, 10)
      assert.equal(slice.tree.children.length, 10)
      assert.equal(slice.omittedCount, 120)

      const children = await api.getChildren({
        scanId: scan.scanId,
        nodeId: ready.rootIds[0],
        offset: 10,
        limit: 7,
        sort: 'size',
      })

      assert.equal(children.offset, 10)
      assert.equal(children.limit, 7)
      assert.equal(children.items.length, 7)
      assert.equal(children.total, 130)
    } finally {
      channel.destroy()
      running.server.close()
    }
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

async function waitForRpcProgress(api: RpcAPI, scanId: string): Promise<ScanStatusResponse> {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    const status = await api.getScanStatus(scanId)
    if (status.currentPath) return status
    await new Promise((resolve) => setTimeout(resolve, 10))
  }
  throw new Error(`Scan ${scanId} did not report progress`)
}

async function waitForRpcReady(api: RpcAPI, scanId: string): Promise<ScanStatusResponse> {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    const status = await api.getScanStatus(scanId)
    if (status.state === 'ready') return status
    await new Promise((resolve) => setTimeout(resolve, 10))
  }
  throw new Error(`Scan ${scanId} did not become ready`)
}

function withTimeout<T>(promise: Promise<T>, ms = 500): Promise<T> {
  return Promise.race([
    promise,
    new Promise<never>((_resolve, reject) => {
      setTimeout(() => reject(new Error(`Timed out after ${ms}ms`)), ms)
    }),
  ])
}
