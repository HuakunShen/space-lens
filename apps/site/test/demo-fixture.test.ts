import assert from 'node:assert/strict'
import { test } from 'node:test'
import { MemorySnapshotSchema, TreeSliceNodeSchema } from '@space-lens/contract'
import { makeDemoMemorySnapshot, makeDemoTree, findDemoNode } from '../src/lib/demo-fixture.ts'

test('sample disk usage is a valid nested contract tree with internally consistent byte totals', () => {
  const tree = makeDemoTree()
  assert.equal(TreeSliceNodeSchema.safeParse(tree).success, true)
  assert.equal(tree.size, tree.children.reduce((size, child) => size + child.size, 0))
  assert.equal(tree.depth, 0)
  assert.ok(tree.children.length >= 4)
  assert.equal(findDemoNode(tree, tree.children[0]!.id)?.id, tree.children[0]!.id)
  assert.equal(findDemoNode(tree, 'no-real-path'), null)
  assert.ok(tree.children.every((child) => child.path.startsWith('/demo/')))
})

test('memory snapshot uses a schema-valid safe, fictional sample', () => {
  const memory = makeDemoMemorySnapshot()
  assert.equal(MemorySnapshotSchema.safeParse(memory).success, true)
  assert.equal(memory.groups.length, 5)
  assert.equal(memory.processes.length, 5)
  assert.ok(memory.groups.every((group) => group.mainPid >= 1200))
  assert.equal(memory.breakdown.usedBytes + memory.breakdown.freeBytes, memory.breakdown.totalBytes)
})
