import assert from 'node:assert/strict'
import { test } from 'node:test'
import { addSelection, isPathWithin, selectedAncestor } from '../src/lib/selection.ts'
import type { CollectorEntry, TreeNodeSummary } from '@space-lens/contract'
const node = (path: string): TreeNodeSummary => ({
  id: path,
  name: path.split(/[\\/]/).at(-1)!,
  path,
  size: 10,
  depth: 1,
  ignored: false,
  collapsed: false,
  hasChildren: false,
  childCount: 0,
})
const add = (entries: CollectorEntry[], path: string) =>
  addSelection(entries, node(path), 'scan_12345678', '2026-10-02T00:00:00.000Z', path)
test('a selected parent supersedes descendants and prevents double counting', () => {
  let entries = add([], '/root/cache/file')
  entries = add(entries, '/root/cache')
  entries = add(entries, '/root/cache/another')
  entries = add(entries, '/root/cache')
  assert.deepEqual(
    entries.map((e) => e.path),
    ['/root/cache'],
  )
  assert.equal(selectedAncestor(entries, '/root/cache/another')?.path, '/root/cache')
})
test('prefix siblings remain independently selectable', () => {
  assert.equal(isPathWithin('/root/cache2/file', '/root/cache'), false)
  assert.equal(add(add([], '/root/cache'), '/root/cache2').length, 2)
})
test('Windows paths compare separators and case without treating POSIX paths as insensitive', () => {
  assert.equal(isPathWithin('C:\\Code\\Cache\\file', 'c:/code/cache/'), true)
  assert.equal(add(add([], 'C:\\Code\\Cache'), 'c:/code/cache/file').length, 1)
  assert.equal(isPathWithin('/Root/cache/file', '/root/cache'), false)
})
test('root paths retain their boundary semantics', () => {
  assert.equal(isPathWithin('/file', '/'), true)
  assert.equal(isPathWithin('C:\\file', 'C:\\'), true)
  assert.equal(isPathWithin('D:\\file', 'C:\\'), false)
})

test('skipped and partially scanned directories cannot enter cleanup review', () => {
  for (const scanState of ['skipped', 'partial'] as const) {
    const unavailable = { ...node('/root/cloud'), scanState, isDirectory: true, skipReason: 'cloud' }
    assert.deepEqual(addSelection([], unavailable, 'scan_12345678', '2026-10-02T00:00:00.000Z', 'bad'), [])
  }
  const complete = { ...node('/root/local'), scanState: 'complete' as const }
  assert.equal(addSelection([], complete, 'scan_12345678', '2026-10-02T00:00:00.000Z', 'ok').length, 1)
})
