import { createHash } from 'node:crypto'
import { describe, expect, it } from 'vitest'

import { nodeIdOf, ScanIndex } from '../src/scan-index.ts'

const node = (path: string, size = 0) => ({ path, size, ignored: false, collapsed: false })

describe('scan index', () => {
  it('keeps sha1(path) wire ids and rebuilds paths from parent links', () => {
    const index = new ScanIndex(16)
    const root = index.append(node('/'), -1, null)
    const users = index.append(node('/Users', 10), root, '/')
    const file = index.append(node('/Users/中文 file.txt', 7), users, '/Users')
    expect(index.idOf(file)).toBe(createHash('sha1').update('/Users/中文 file.txt').digest('hex').slice(0, 24))
    expect(nodeIdOf('/Users')).toBe(index.idOf(users))
    expect(index.pathOf(users)).toBe('/Users')
    expect(index.pathOf(file)).toBe('/Users/中文 file.txt')
    expect(index.baseNameOf(root)).toBe('')
    expect(index.summarize(file)).toEqual({
      id: index.idOf(file),
      name: '中文 file.txt',
      path: '/Users/中文 file.txt',
      size: 7,
      depth: 2,
      ignored: false,
      collapsed: false,
      hasChildren: false,
      childCount: 0,
    })
    expect(index.find(index.idOf(file))).toBe(file)
    expect(index.find(index.idOf(file).toUpperCase())).toBe(-1)
    expect(index.find('nope')).toBe(-1)
    expect(index.roots).toEqual([root])
  })

  it('stores a path that does not extend its parent, and names that are not valid UTF-16', () => {
    const index = new ScanIndex(16)
    const root = index.append(node('/data'), -1, null)
    const odd = index.append(node('/elsewhere/child'), root, '/data')
    const lone = index.append(node('/data/bad\ud800name'), root, '/data')
    expect(index.pathOf(odd)).toBe('/elsewhere/child')
    expect(index.baseNameOf(odd)).toBe('child')
    expect(index.pathOf(lone)).toBe('/data/bad\ud800name')
  })

  it('rejects duplicate paths and keeps children in reported order across growth and sealing', () => {
    const index = new ScanIndex(16)
    const root = index.append(node('/r'), -1, null)
    const children = Array.from({ length: 100 }, (_, position) => index.append(node(`/r/c${position}`), root, '/r'))
    expect(index.append(node('/r/c5'), root, '/r')).toBe(-1)
    index.seal()
    expect(index.childrenOf(root)).toEqual(children)
    expect(index.childCount(root)).toBe(100)
    const late = index.append(node('/r/late'), root, '/r')
    expect(index.childrenOf(root).at(-1)).toBe(late)
    for (const child of children) expect(index.find(index.idOf(child))).toBe(child)
  })

  it('records local metadata only when the scanner reported it', () => {
    const index = new ScanIndex(16)
    const root = index.append(
      { ...node('/r', 9), logicalSize: 4, isDirectory: true, scanState: 'partial', skipReason: null },
      -1,
      null,
    )
    const skipped = index.append(
      { ...node('/r/s'), logicalSize: 99, isDirectory: true, scanState: 'skipped', skipReason: 'dataless' },
      root,
      '/r',
    )
    expect(index.summarize(root)).toMatchObject({
      logicalSize: 4,
      isDirectory: true,
      scanState: 'partial',
      skipReason: null,
    })
    expect(index.summarize(skipped)).toMatchObject({ scanState: 'skipped', skipReason: 'dataless', logicalSize: 99 })
    const plain = new ScanIndex(16)
    expect(Object.keys(plain.summarize(plain.append(node('/p'), -1, null)))).not.toContain('scanState')
  })

  it('adds discovered nodes without changing the original children, and overrides cleanup values', () => {
    const index = new ScanIndex(16)
    const fingerprint = { size: 1, mtimeMs: 2 }
    const root = index.append({ ...node('/r'), fingerprint }, -1, null)
    const kept = index.append({ ...node('/r/kept', 5), fingerprint }, root, '/r')
    const hidden = index.append({ ...node('/r/.hidden', 3), fingerprint }, root, '/r', true)
    const inner = index.append({ ...node('/r/.hidden/inner', 3), fingerprint }, hidden, '/r/.hidden', true)
    expect(index.childrenOf(root)).toEqual([kept])
    expect(index.childrenOf(hidden)).toEqual([inner])
    expect(index.depthOf(inner)).toBe(2)
    expect(index.roots).toEqual([root])
    expect(index.cleanupView(kept)).toEqual({ path: '/r/kept', size: 5, ignored: false, fingerprint })
    index.setCleanup(kept, 8, true, { size: 3, mtimeMs: 4 })
    expect(index.cleanupView(kept)).toEqual({
      path: '/r/kept',
      size: 8,
      ignored: true,
      fingerprint: { size: 3, mtimeMs: 4 },
    })
    expect(index.summarize(kept).size).toBe(5)
    const unfingerprinted = new ScanIndex(16)
    expect(unfingerprinted.cleanupView(unfingerprinted.append(node('/u'), -1, null)).fingerprint).toBeUndefined()
  })
})
