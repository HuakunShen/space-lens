import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { CollectorEntry, TreeNodeSummary } from '@space-lens/contract'
import { createStager } from '../src/stage.ts'

const node = (id: string, scanState: string): TreeNodeSummary =>
  ({ id, name: id, path: `/scan/${id}`, size: 1000, kind: 'directory', depth: 1, scanState }) as unknown as TreeNodeSummary

function stagerFor(nodes: TreeNodeSummary[]) {
  let collector: CollectorEntry[] = []
  const stager = createStager({
    scanId: 'scan_test',
    resolve: async (id) => nodes.find((n) => n.id === id) ?? null,
    collector: () => collector,
    setCollector: (entries) => {
      collector = entries
    },
    newId: () => `col_${collector.length}`,
  })
  return { stager, entries: () => collector }
}

test('a partial folder is refused with a hint to stage its children', async () => {
  const { stager, entries } = stagerFor([node('n_partial', 'partial'), node('n_ok', 'complete')])
  const outcome = await stager.stage([
    { nodeId: 'n_partial', reason: 'cache', risk: 'regenerable' },
    { nodeId: 'n_ok', reason: 'cache', risk: 'regenerable' },
  ])
  assert.equal(entries().length, 1)
  const refusal = outcome.rejected.find((r) => r.nodeId === 'n_partial')
  assert.ok(refusal)
  assert.match(refusal.reason, /list_children/)
  assert.match(refusal.reason, /n_partial/)
})
