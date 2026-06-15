import assert from 'node:assert/strict'
import { once } from 'node:events'
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import type { AddressInfo } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'

import { RPCChannel } from 'kkrpc'
import { webSocketClientTransport } from 'kkrpc/ws'

import { createWebApp, startWebServer } from './web-server.js'
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

interface RpcAPI {
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
    const channel = new RPCChannel<object, RpcAPI>(
      webSocketClientTransport({ url: `ws://127.0.0.1:${address.port}/rpc` }),
      { timeout: 500 },
    )
    const api = channel.getAPI()

    try {
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

test('web server deletes manually collected paths', async () => {
  const root = mkdtempSync(join(tmpdir(), 'space-lens-web-manual-delete-'))

  try {
    const target = join(root, 'delete-me.txt')
    writeFileSync(target, 'delete me')
    const app = createWebApp({ staticDir: root })

    const response = await app.request('/api/cleanup/execute', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
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
      }),
    })

    assert.equal(response.status, 200)
    const outcome = (await response.json()) as CleanupOutcomeResponse
    assert.deepEqual(outcome.errors, [])
    assert.equal(outcome.removed.length, 1)
    assert.equal(outcome.removed[0].path, target)
    assert.equal(outcome.bytesRemoved, 9)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('web server starts scans in the background and reports progress status', async () => {
  const root = mkdtempSync(join(tmpdir(), 'space-lens-web-progress-'))

  try {
    writeFileSync(join(root, 'alpha.txt'), 'alpha')
    let finishScan!: (data: SpaceLensData) => void
    const app = createWebApp({
      staticDir: root,
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

    const scanResponse = await app.request('/api/scans', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        paths: [root],
        ignoreHidden: false,
        respectGitignore: false,
        ignoredMode: 'summarize',
        initialDepth: 1,
        maxChildrenPerNode: 10,
      }),
    })
    assert.equal(scanResponse.status, 200)
    const scan = (await scanResponse.json()) as ScanResponse

    const scanningResponse = await app.request(`/api/scans/${scan.scanId}/status`)
    assert.equal(scanningResponse.status, 200)
    const scanning = (await scanningResponse.json()) as ScanStatusResponse
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

    await waitForReady(app, scan.scanId)
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

    const app = createWebApp({ staticDir: root })
    const scanResponse = await app.request('/api/scans', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        paths: [root],
        ignoreHidden: false,
        respectGitignore: false,
        ignoredMode: 'summarize',
        initialDepth: 1,
        maxChildrenPerNode: 10,
      }),
    })
    assert.equal(scanResponse.status, 200)

    const scan = (await scanResponse.json()) as ScanResponse
    const ready = await waitForReady(app, scan.scanId)
    const nodeResponse = await app.request(`/api/scans/${scan.scanId}/node`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        scanId: scan.scanId,
        nodeId: ready.rootIds[0],
        depth: 1,
        maxChildrenPerNode: 10,
      }),
    })
    assert.equal(nodeResponse.status, 200)

    const slice = (await nodeResponse.json()) as TreeSliceResponse
    assert.equal(slice.children.length, 10)
    assert.equal(slice.tree.children.length, 10)
    assert.equal(slice.omittedCount, 120)

    const childrenResponse = await app.request(`/api/scans/${scan.scanId}/children`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        scanId: scan.scanId,
        nodeId: ready.rootIds[0],
        offset: 10,
        limit: 7,
        sort: 'size',
      }),
    })
    assert.equal(childrenResponse.status, 200)

    const children = (await childrenResponse.json()) as ChildrenPageResponse
    assert.equal(children.offset, 10)
    assert.equal(children.limit, 7)
    assert.equal(children.items.length, 7)
    assert.equal(children.total, 130)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

async function waitForReady(app: ReturnType<typeof createWebApp>, scanId: string): Promise<ScanStatusResponse> {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    const response = await app.request(`/api/scans/${scanId}/status`)
    assert.equal(response.status, 200)
    const status = (await response.json()) as ScanStatusResponse
    if (status.state === 'ready') return status
    await new Promise((resolve) => setTimeout(resolve, 10))
  }
  throw new Error(`Scan ${scanId} did not become ready`)
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
