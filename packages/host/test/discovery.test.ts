import { mkdtempSync, mkdirSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, parse } from 'node:path'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { ScanManager } from '../src/scan-store.ts'
import { EventRing } from '../src/events.ts'
import type { TrashPort } from '../src/trash.ts'

describe('background discovery', () => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'lens-discovery-')))
  const outside = realpathSync(mkdtempSync(join(tmpdir(), 'lens-external-')))
  const trashed: string[] = []
  const trash: TrashPort = {
    async trash(paths) {
      for (const path of paths) {
        trashed.push(path)
        rmSync(path, { recursive: true })
      }
      return new Map(paths.map((path) => [path, null]))
    },
  }
  const manager = new ScanManager({
    events: new EventRing(),
    trash,
    roots: [root],
    cleanupMode: 'trash',
    maxConcurrent: 2,
    maxTotal: 8,
  })
  let scanId: string
  let rootId: string
  const file = (path: string, size = 4096) => {
    mkdirSync(join(path, '..'), { recursive: true })
    writeFileSync(path, Buffer.alloc(size, 1))
  }
  const discover = (kind: 'large-files' | 'caches' | 'gitignored', extra = {}) =>
    manager.discover({ scanId, kind, minSize: 0, offset: 0, limit: 200, ...extra })

  beforeAll(async () => {
    file(join(root, 'project', 'package.json'))
    file(join(root, 'project', 'Cargo.toml'))
    file(join(root, 'project', 'node_modules', 'lib', '__pycache__', 'nested.pyc'))
    file(join(root, 'project', 'target', 'debug', 'output'))
    file(join(root, 'project', '.next', 'cache', 'output'))
    file(join(root, 'python', '__pycache__', 'output.pyc'))
    file(join(root, 'python', '.pytest_cache', 'output'))
    file(join(root, 'python', '.mypy_cache', 'output'))
    file(join(root, 'python', '.ruff_cache', 'output'))
    file(join(root, 'fake', 'target', 'personal-file'))
    file(join(root, 'fake', 'node_modules', 'personal-file'))
    file(join(root, 'fake', '.next', 'personal-file'))
    file(join(root, '.venv', '__pycache__', 'dont-clean.pyc'))
    file(join(root, 'custom-env', 'pyvenv.cfg'))
    file(join(root, 'custom-env', '__pycache__', 'dont-clean.pyc'))
    file(join(root, 'uv', 'cache', 'personal-file'))
    for (const inheritedName of ['constructor', 'toString', '__proto__']) {
      file(join(root, inheritedName, 'personal-file'))
    }
    file(join(root, 'ignored', 'deep', 'largest.bin'), 16384)
    file(join(root, 'visible.bin'), 8192)
    file(join(outside, 'escaped.bin'), 32768)
    symlinkSync(outside, join(root, 'escape'))
    symlinkSync(join(outside, 'escaped.bin'), join(root, 'escape-file'))
    writeFileSync(join(root, '.gitignore'), 'ignored/\nproject/node_modules/\nproject/target/\n')
    const session = await manager.start(
      { paths: [root, join(root, 'project')], ignoreHidden: false, respectGitignore: true, ignoredMode: 'summarize' },
      undefined,
    )
    scanId = session.scanId
    await manager.wait(scanId)
    rootId = manager.status(scanId).rootIds[0]
  })

  it('finds deep ignored files with stable selectable IDs without changing the chart', async () => {
    const before = manager.slice(scanId, rootId, 24, 512)
    const page = await discover('large-files', { minSize: 8192, limit: 1 })
    expect(page.items.map((item) => item.node.path)).toEqual([join(root, 'ignored', 'deep', 'largest.bin')])
    expect(page.items[0].node.ignored).toBe(true)
    expect(page.items[0].isDirectory).toBe(false)
    expect(page.items[0].parentId).not.toBeNull()
    expect(page.total).toBe(2)
    expect(page.totalSize).toBe(24576)
    // Later paging reads the snapshot rather than scanning the filesystem again.
    file(join(root, 'created-after-discovery.bin'), 32768)
    const second = await discover('large-files', { minSize: 8192, offset: 1, limit: 1 })
    expect(second.items.map((item) => item.node.path)).toEqual([join(root, 'visible.bin')])
    expect(second.total).toBe(2)
    expect(second.totalSize).toBe(24576)
    expect((await discover('large-files', { minSize: 16385 })).total).toBe(0)
    expect(manager.slice(scanId, rootId, 24, 512).tree).toEqual(before.tree)
    expect((await discover('large-files', { minSize: 8192 })).items[0].node.id).toBe(page.items[0].node.id)
    expect(manager.status(scanId).rootIds).toHaveLength(1)
  })

  it('recognizes project and Python caches, pruning nested candidates and virtual environments', async () => {
    const page = await discover('caches')
    expect(page.items.map((item) => item.node.path).sort()).toEqual(
      [
        join(root, 'project', 'node_modules'),
        join(root, 'project', 'target'),
        join(root, 'project', '.next'),
        join(root, 'python', '__pycache__'),
        join(root, 'python', '.pytest_cache'),
        join(root, 'python', '.mypy_cache'),
        join(root, 'python', '.ruff_cache'),
      ].sort(),
    )
    expect(page.items.every((item) => item.isDirectory)).toBe(true)
    expect(page.items.find((item) => item.node.name === 'target')?.category).toBe('Rust build output')
    const first = await discover('caches', { limit: 2 })
    expect(first.total).toBe(7)
    expect(first.totalSize).toBe(page.totalSize)
    expect(first.items).toHaveLength(2)
    expect((await discover('caches', { minSize: page.items[0].node.size + 1 })).total).toBeLessThan(7)
  })

  it('returns non-overlapping gitignored candidates even when the original scan excluded ignored paths', async () => {
    const page = await discover('gitignored')
    expect(page.items.map((item) => item.node.path).sort()).toEqual(
      [join(root, 'ignored'), join(root, 'project', 'node_modules'), join(root, 'project', 'target')].sort(),
    )
    const excluded = await manager.start(
      { paths: [root], ignoreHidden: false, respectGitignore: true, ignoredMode: 'exclude' },
      undefined,
    )
    await manager.wait(excluded.scanId)
    const result = await manager.discover({
      scanId: excluded.scanId,
      kind: 'gitignored',
      minSize: 0,
      offset: 0,
      limit: 200,
    })
    expect(result.items.map((item) => item.node.path).sort()).toEqual(page.items.map((item) => item.node.path).sort())
  })

  it('plans discovered nodes once, normalizes descendants, and executes only through Trash', async () => {
    const files = await discover('large-files', { minSize: 8192 })
    const ignored = (await discover('gitignored')).items.find((item) => item.node.name === 'ignored')!
    const plan = manager.plan(scanId, [files.items[0].node.id, ignored.node.id, ignored.node.id])
    expect(plan.entries.map((entry) => entry.path)).toEqual([join(root, 'ignored')])
    expect(plan.totalSize).toBe(ignored.node.size)
    const outcome = await manager.execute(plan.planId)
    expect(outcome.trashed.map((entry) => entry.path)).toEqual([join(root, 'ignored')])
    expect(trashed).toEqual([join(root, 'ignored')])
  })

  it('never offers symlink escapes and refuses roots or replaced symlink ancestors for cleanup', async () => {
    const page = await discover('large-files')
    expect(page.items.some((item) => item.node.path.includes('escape'))).toBe(false)
    expect(() => manager.plan(scanId, [rootId])).toThrow()
    const cache = (await discover('caches')).items.find((item) => item.node.name === '__pycache__')!
    const plan = manager.plan(scanId, [cache.node.id])
    rmSync(join(root, 'python'), { recursive: true })
    symlinkSync(outside, join(root, 'python'))
    expect(() => manager.plan(scanId, [cache.node.id])).toThrow()
    await expect(manager.execute(plan.planId)).rejects.toMatchObject({ problem: { code: 'StalePlan' } })
    expect(trashed).toHaveLength(1)
  })

  it('contains folder scans when the configured root is the filesystem root', async () => {
    const filesystem = new ScanManager({
      events: new EventRing(),
      trash,
      roots: [parse(root).root],
      cleanupMode: 'none',
      maxConcurrent: 1,
      maxTotal: 1,
    })
    const session = await filesystem.start(
      { paths: [root], ignoreHidden: false, respectGitignore: true, ignoredMode: 'summarize' },
      undefined,
    )
    await filesystem.wait(session.scanId)
    expect(filesystem.status(session.scanId).state).toBe('ready')
  })

  afterAll(() => {
    rmSync(root, { recursive: true, force: true })
    rmSync(outside, { recursive: true, force: true })
  })
})
