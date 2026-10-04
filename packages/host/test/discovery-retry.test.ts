import { mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it, vi } from 'vitest'
import { ScanManager } from '../src/scan-store.ts'
import { EventRing } from '../src/events.ts'

const faults = vi.hoisted(() => ({ failNextDiscovery: false }))
vi.mock('node:worker_threads', async (importOriginal) => {
  const actual = await importOriginal<typeof import('node:worker_threads')>()
  return {
    ...actual,
    Worker: class extends actual.Worker {
      constructor(source: string | URL, options?: import('node:worker_threads').WorkerOptions) {
        if (faults.failNextDiscovery && options?.workerData?.roots) {
          faults.failNextDiscovery = false
          // Exercise a genuine worker startup error while keeping subsequent
          // attempts and all native filesystem inspection real.
          super('throw new Error("transient worker startup failure")', { eval: true })
        } else super(source, options)
      }
    },
  }
})

describe('discovery retry', () => {
  it('retries a failed worker for the same scan after concurrent requests fail', async () => {
    const root = realpathSync(mkdtempSync(join(tmpdir(), 'lens-retry-')))
    try {
      writeFileSync(join(root, 'large.bin'), Buffer.alloc(16384, 1))
      const manager = new ScanManager({
        events: new EventRing(),
        trash: { trash: async () => new Map() },
        roots: [root],
        cleanupMode: 'none',
        maxConcurrent: 1,
        maxTotal: 1,
      })
      const scan = await manager.start(
        { paths: [root], ignoreHidden: false, respectGitignore: true, ignoredMode: 'summarize' },
        undefined,
      )
      await manager.wait(scan.scanId)
      const request = { scanId: scan.scanId, kind: 'large-files' as const, minSize: 8192, offset: 0, limit: 1 }
      faults.failNextDiscovery = true
      const attempts = await Promise.allSettled([manager.discover(request), manager.discover(request)])
      expect(attempts).toEqual([
        expect.objectContaining({
          status: 'rejected',
          reason: expect.objectContaining({ problem: expect.objectContaining({ code: 'Unavailable' }) }),
        }),
        expect.objectContaining({
          status: 'rejected',
          reason: expect.objectContaining({ problem: expect.objectContaining({ code: 'Unavailable' }) }),
        }),
      ])
      const recovered = await manager.discover(request)
      expect(recovered.total).toBe(1)
      expect(recovered.items[0].node.name).toBe('large.bin')
      expect((await manager.discover(request)).items[0].node.id).toBe(recovered.items[0].node.id)
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })
})
