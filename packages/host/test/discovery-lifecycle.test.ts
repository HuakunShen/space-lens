import { mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it, vi } from 'vitest'
import { ScanManager } from '../src/scan-store.ts'
import { EventRing } from '../src/events.ts'

const workers = vi.hoisted(() => ({ active: 0, maximum: 0, started: 0, activeAll: 0, maximumAll: 0 }))
vi.mock('node:worker_threads', async (importOriginal) => {
  const actual = await importOriginal<typeof import('node:worker_threads')>()
  return {
    ...actual,
    Worker: class extends actual.Worker {
      constructor(source: string | URL, options?: import('node:worker_threads').WorkerOptions) {
        const discovery = Boolean(options?.workerData?.roots)
        // Slow the genuine worker's start enough to exercise overlapping callers
        // deterministically; filesystem inspection and worker exit remain real.
        super(discovery ? `setTimeout(() => { ${String(source)} }, 100)` : source, options)
        workers.maximumAll = Math.max(workers.maximumAll, ++workers.activeAll)
        this.once('exit', () => {
          workers.activeAll -= 1
        })
        if (discovery) {
          workers.started += 1
          workers.maximum = Math.max(workers.maximum, ++workers.active)
          this.once('exit', () => {
            workers.active -= 1
          })
        }
      }
    },
  }
})

async function fixture() {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'lens-lifecycle-')))
  writeFileSync(join(root, 'file.bin'), Buffer.alloc(16384, 1))
  const manager = new ScanManager({
    events: new EventRing(),
    trash: { trash: async () => new Map() },
    roots: [root],
    cleanupMode: 'none',
    maxConcurrent: 1,
    maxTotal: 3,
  })
  const request = { paths: [root], ignoreHidden: false, respectGitignore: true, ignoredMode: 'summarize' as const }
  const first = await manager.start(request, undefined)
  await manager.wait(first.scanId)
  const second = await manager.start(request, undefined)
  await manager.wait(second.scanId)
  const discover = (scanId: string) =>
    manager.discover({ scanId, kind: 'large-files', minSize: 8192, offset: 0, limit: 1 })
  return { root, manager, first, second, request, discover }
}

describe('discovery lifecycle', () => {
  it('queues discovery until an existing native scan releases its worker', async () => {
    workers.maximumAll = 0
    const setup = await fixture()
    try {
      const third = await setup.manager.start(setup.request, undefined)
      const started = workers.started
      const pending = setup.discover(setup.first.scanId)
      expect(workers.started).toBe(started)
      await setup.manager.wait(third.scanId)
      expect((await pending).total).toBe(1)
      expect(workers.active).toBe(0)
      expect(workers.maximumAll).toBe(1)
    } finally {
      await setup.manager.close()
      rmSync(setup.root, { recursive: true, force: true })
    }
  })

  it('waits for a cancelled scan worker to exit before launching queued discovery', async () => {
    workers.maximumAll = 0
    const setup = await fixture()
    try {
      const third = await setup.manager.start(setup.request, undefined)
      const pending = setup.discover(setup.first.scanId)
      setup.manager.cancel(third.scanId)
      expect((await pending).total).toBe(1)
      expect(setup.manager.status(third.scanId).state).toBe('cancelled')
      expect(workers.maximumAll).toBe(1)
      expect(workers.activeAll).toBe(0)
    } finally {
      await setup.manager.close()
      rmSync(setup.root, { recursive: true, force: true })
    }
  })
  it('bounds discovery workers and counts them against new native scans', async () => {
    workers.maximum = 0
    const setup = await fixture()
    try {
      const first = setup.discover(setup.first.scanId)
      const second = setup.discover(setup.second.scanId)
      await expect(setup.manager.start(setup.request, undefined)).rejects.toMatchObject({
        problem: { code: 'LimitExceeded' },
      })
      const pages = await Promise.all([first, second])
      expect(pages.map((page) => page.total)).toEqual([1, 1])
      expect(workers.maximum).toBe(1)
      expect(workers.active).toBe(0)
    } finally {
      await setup.manager.close()
      rmSync(setup.root, { recursive: true, force: true })
    }
  })

  it('terminates active discovery and settles queued requests when the host closes', async () => {
    const setup = await fixture()
    try {
      const started = workers.started
      const requests = Promise.allSettled([setup.discover(setup.first.scanId), setup.discover(setup.second.scanId)])
      await setup.manager.close()
      const results = await requests
      expect(results.every((result) => result.status === 'rejected')).toBe(true)
      expect(workers.active).toBe(0)
      expect(workers.started - started).toBe(1)
      await expect(setup.discover(setup.first.scanId)).rejects.toMatchObject({ problem: { code: 'Unavailable' } })
      await setup.manager.close()
    } finally {
      await setup.manager.close()
      rmSync(setup.root, { recursive: true, force: true })
    }
  })
})
