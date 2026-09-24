import { mkdtempSync, mkdirSync, realpathSync, rmSync, utimesSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { join, relative } from 'node:path'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'

import { ScanManager } from '../src/scan-store.ts'
import { EventRing } from '../src/events.ts'
import { ProblemError } from '../src/problems.ts'
import type { TrashPort } from '../src/trash.ts'
import type { DirectoryNode } from 'space-lens'

function findNode(tree: DirectoryNode[], name: string): DirectoryNode {
  const stack = [...tree]
  while (stack.length > 0) {
    const node = stack.pop()!
    if (node.name === name) return node
    for (const child of node.children ?? []) stack.push(child)
  }
  throw new Error(`node ${name} not found`)
}

function fakeTrash(targetDir: string): TrashPort & { trashed: string[] } {
  let counter = 0
  return {
    trashed: [],
    async trash(paths) {
      const results = new Map<string, string | null>()
      for (const path of paths) {
        counter += 1
        const destination = join(targetDir, `.trashed-${counter}`)
        rmSync(path, { recursive: true })
        writeFileSync(destination, '')
        this.trashed.push(path)
        results.set(path, null)
      }
      return results
    },
  }
}

describe('scan manager', () => {
  // realpath: on macOS tmpdir (/var/...) is a symlink to /private/var/... and
  // the engine receives the resolved absolute path
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-host-')))
  const bigDir = join(root, 'big')
  const smallDir = join(root, 'small')
  const ignoredDir = join(root, 'ignored-dir')
  beforeAll(() => {
    mkdirSync(bigDir)
    mkdirSync(smallDir)
    mkdirSync(ignoredDir)
    writeFileSync(join(bigDir, 'large.bin'), Buffer.alloc(4096))
    writeFileSync(join(smallDir, 'tiny.txt'), 'hello')
    writeFileSync(join(ignoredDir, 'junk.txt'), 'junk')
    writeFileSync(join(root, '.gitignore'), 'ignored-dir\n')
    const old = new Date(Date.now() - 60_000)
    utimesSync(join(bigDir, 'large.bin'), old, old)
  })

  const events = new EventRing()
  const trash = fakeTrash(root)
  let manager: ScanManager

  it('runs a scan against the real engine and indexes it', async () => {
    manager = new ScanManager({
      events,
      trash,
      roots: [root],
      cleanupMode: 'trash',
      maxConcurrent: 2,
      maxTotal: 4,
    })
    const session = await manager.start(
      { paths: [root], ignoreHidden: false, respectGitignore: true, ignoredMode: 'summarize' },
      'fixture',
    )
    expect(session.scanId.startsWith('scan_')).toBe(true)
    await manager.wait(session.scanId)
    const status = manager.status(session.scanId)
    expect(status.state).toBe('ready')
    expect(status.rootIds.length).toBe(1)
    expect(status.progress).toBeNull()
    // ignored dir is summarized, not traversed
    const bigNode = findNodeFromManager(manager, session.scanId, bigDir)
    expect(bigNode.ignored).toBe(false)
  })

  it('serves tree slices with truncation accounting', async () => {
    const [status] = manager.list()
    const slice = manager.slice(status.scanId, status.rootIds[0], 1, 1)
    expect(slice.focusNode.path).toBe(root)
    expect(slice.ancestors).toEqual([])
    // only one child kept at depth 1, the other becomes omitted
    expect(slice.tree.children.length).toBe(1)
    expect(slice.omittedCount).toBeGreaterThan(0)
    expect(slice.omittedBytes).toBeGreaterThan(0)
    expect(slice.truncated).toBe(true)
  })

  it('pages children in every sort mode', async () => {
    const [status] = manager.list()
    const page = manager.children(status.scanId, status.rootIds[0], 0, 1, 'size')
    expect(page.total).toBeGreaterThanOrEqual(2)
    expect(page.items.length).toBe(1)
    expect(page.sort).toBe('size')
    const byName = manager.children(status.scanId, status.rootIds[0], 0, 10, 'name')
    const names = byName.items.map((item) => item.name)
    expect(names).toEqual([...names].sort((a, b) => a.localeCompare(b)))
  })

  it('plans from node ids with fingerprints and refuses unknown ids', async () => {
    const [status] = manager.list()
    const bigId = idOf(manager, status.scanId, bigDir)
    const plan = manager.plan(status.scanId, [bigId])
    expect(plan.mode).toBe('trash')
    expect(plan.entries[0].path).toBe(bigDir)
    expect(plan.entries[0].fingerprint.size).toBeGreaterThan(0)
    expect(() => manager.plan(status.scanId, ['nope'])).toThrowError(ProblemError)
  })

  it('executes plans through the trash port', async () => {
    const [status] = manager.list()
    const bigId = idOf(manager, status.scanId, bigDir)
    const plan = manager.plan(status.scanId, [bigId])
    const outcome = await manager.execute(plan.planId)
    expect(outcome.trashed.map((entry) => entry.path)).toEqual([bigDir])
    expect(outcome.failed).toEqual([])
    expect(trash.trashed).toContain(bigDir)
    await expect(manager.execute(plan.planId)).rejects.toMatchObject({ problem: { code: 'NotFound' } })
  })

  it('fails closed when content changed after planning', async () => {
    const [status] = manager.list()
    const tinyId = idOf(manager, status.scanId, join(smallDir, 'tiny.txt'))
    const plan = manager.plan(status.scanId, [tinyId])
    const later = new Date(Date.now() + 5_000)
    utimesSync(join(smallDir, 'tiny.txt'), later, later)
    await expect(manager.execute(plan.planId)).rejects.toMatchObject({ problem: { code: 'StalePlan' } })
  })

  it('refuses paths outside the served roots', async () => {
    await expect(
      manager.start(
        { paths: ['/usr'], ignoreHidden: false, respectGitignore: true, ignoredMode: 'summarize' },
        undefined,
      ),
    ).rejects.toMatchObject({
      problem: { code: 'Forbidden' },
    })
  })

  it('expands a leading ~ against the process home before containing it', async () => {
    // `~/<relative path to the fixture>` must land inside the served root
    // exactly like its absolute spelling, while a bare `~` (home itself) is
    // still outside the roots and refused.
    const homeRelative = relative(homedir(), root)
    const session = await manager.start(
      {
        paths: [`~/${homeRelative}`],
        ignoreHidden: false,
        respectGitignore: true,
        ignoredMode: 'summarize',
      },
      'tilde',
    )
    await manager.wait(session.scanId)
    expect(manager.status(session.scanId).state).toBe('ready')

    await expect(
      manager.start({ paths: ['~'], ignoreHidden: false, respectGitignore: true, ignoredMode: 'summarize' }, undefined),
    ).rejects.toMatchObject({ problem: { code: 'Forbidden' } })
  })

  it('publishes contract-valid events for the scan lifecycle', () => {
    const kinds = events.replay(0).events.map((envelope) => envelope.payload.kind)
    expect(kinds).toContain('scan.updated')
    expect(kinds).toContain('scan.completed')
  })

  it('rejects cleanup when the host is read-only', async () => {
    const readOnly = new ScanManager({
      events,
      trash,
      roots: [root],
      cleanupMode: 'none',
      maxConcurrent: 1,
      maxTotal: 4,
    })
    const session = await readOnly.start(
      { paths: [root], ignoreHidden: false, respectGitignore: true, ignoredMode: 'summarize' },
      undefined,
    )
    await readOnly.wait(session.scanId)
    const status = readOnly.status(session.scanId)
    let thrown: unknown
    try {
      readOnly.plan(status.scanId, [status.rootIds[0]])
    } catch (error) {
      thrown = error
    }
    expect(thrown).toBeInstanceOf(ProblemError)
    expect((thrown as ProblemError).problem.code).toBe('UnsupportedOperation')
  })

  afterAll(() => {
    rmSync(root, { recursive: true, force: true })
  })
})

function findNodeFromManager(manager: ScanManager, scanId: string, path: string): { ignored: boolean } {
  const status = manager.status(scanId)
  const slice = manager.slice(scanId, status.rootIds[0], 24, 512)
  const stack = [slice.tree]
  while (stack.length > 0) {
    const node = stack.pop()!
    if (node.path === path) return { ignored: node.ignored }
    stack.push(...node.children)
  }
  throw new Error(`node not found in slice: ${path}`)
}

function idOf(manager: ScanManager, scanId: string, path: string): string {
  const status = manager.status(scanId)
  const slice = manager.slice(scanId, status.rootIds[0], 24, 512)
  const stack = [slice.tree]
  while (stack.length > 0) {
    const node = stack.pop()!
    if (node.path === path) return node.id
    stack.push(...node.children)
  }
  throw new Error(`node not found in slice: ${path}`)
}
