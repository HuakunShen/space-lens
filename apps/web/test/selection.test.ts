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

test('coverage is scoped by scanId: entries from another session never cover', () => {
  const scanA = 'scan_aaaaaaaa'
  const scanB = 'scan_bbbbbbbb'
  const at = '2026-10-13T00:00:00.000Z'
  const staged = addSelection([], { ...node('/root/cache'), scanState: 'complete' as const }, scanA, at, 'a1')
  // Same path staged in scan A does not cover scan B.
  assert.equal(selectedAncestor(staged, '/root/cache/file', scanB), undefined)
  assert.equal(selectedAncestor(staged, '/root/cache/file', scanA)?.path, '/root/cache')
  // Legacy callers without a session keep the unfiltered behavior.
  assert.equal(selectedAncestor(staged, '/root/cache/file')?.path, '/root/cache')
  // Staging a parent in scan B prunes only scan B descendants.
  const other = addSelection(staged, { ...node('/root/cache/file'), scanState: 'complete' as const }, scanB, at, 'b1')
  assert.equal(other.length, 2)
  const pruned = addSelection(other, { ...node('/root'), scanState: 'complete' as const }, scanB, at, 'b2')
  assert.deepEqual(
    pruned.map((entry) => `${entry.scanId}:${entry.path}`),
    [`${scanA}:/root/cache`, `${scanB}:/root`],
  )
})

test('synthetic app identifiers are never within, covered, or selectable', () => {
  assert.equal(isPathWithin('app-storage:abcdef', '/'), false)
  assert.equal(isPathWithin('/real/path', 'app-storage:abcdef'), false)
  assert.equal(isPathWithin('app-storage:abcdef', 'app-storage:abcdef'), false)
  assert.equal(selectedAncestor([], 'app-storage:abcdef', 'scan_12345678'), undefined)
  const virtualRoot = {
    ...node('app-storage:abcdef1234567890abcdef'),
    id: 'approot_abcdef1234567890abcdef',
    depth: 0,
    scanState: 'complete' as const,
  }
  assert.deepEqual(addSelection([], virtualRoot, 'scan_12345678', '2026-10-13T00:00:00.000Z', 'v'), [])
  const bundle = {
    ...node('/Applications/Raycast.app'),
    id: 'appbundle_abcdef1234567890abcdef',
    depth: 1,
    scanState: 'complete' as const,
  }
  assert.deepEqual(addSelection([], bundle, 'scan_12345678', '2026-10-13T00:00:00.000Z', 'b'), [])
})

test('the host eligibility predicate overrides the scanState fallback', () => {
  const at = '2026-10-13T00:00:00.000Z'
  // A host-marked cleanable partial stages (A1 per-root evidence).
  const partialCleanable = {
    ...node('/root/nested'),
    scanState: 'partial' as const,
    eligibility: 'cleanable',
  }
  assert.equal(addSelection([], partialCleanable, 'scan_12345678', at, 'ok').length, 1)
  // Contents-only refuses whole-folder staging but is not an error.
  const contentsOnly = { ...node('/root/nested'), scanState: 'partial' as const, eligibility: 'contents-only' }
  assert.deepEqual(addSelection([], contentsOnly, 'scan_12345678', at, 'no'), [])
  // Unknown host values fall back to the scanState rule.
  const unknown = { ...node('/root/x'), scanState: 'complete' as const, eligibility: 'whatever' }
  assert.equal(addSelection([], unknown, 'scan_12345678', at, 'ok').length, 1)
  // Accounting leaves (A12) never stage.
  const accounting = { ...node('/root'), id: 'abc:accounting', scanState: 'complete' as const }
  assert.deepEqual(addSelection([], accounting, 'scan_12345678', at, 'no'), [])
})
