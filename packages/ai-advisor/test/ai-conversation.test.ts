import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { ChildrenPage, CollectorEntry, DiscoveryItem, TreeNodeSummary } from '@space-lens/contract'
import { runAgent } from '../src/agent.ts'
import { createStager } from '../src/stage.ts'
import { createToolRuntime, executeTool, lensPortFromService, type LensService } from '../src/tools.ts'
import type { ChatPort, ChatResponse, LensPort, StageRequestItem } from '../src/types.ts'

// Slice 2: a conversation keeps its seen ids, edits the selection and stays in
// the folder the user has open.

const scanId = 'scan_12345678'

function node(id: string, path: string, size: number, childCount = 0): TreeNodeSummary {
  return {
    id,
    name: path.split('/').at(-1) ?? id,
    path,
    size,
    depth: path.split('/').length - 2,
    ignored: false,
    collapsed: false,
    hasChildren: childCount > 0,
    childCount,
    isDirectory: true,
  }
}

// root > a > b > deep: `deep` is three levels down, outside the initial window.
const root = node('root', '/r', 4_000, 1)
const a = node('a', '/r/a', 4_000, 1)
const b = node('b', '/r/a/b', 4_000, 1)
const deep = node('deep', '/r/a/b/deep', 4_000)
const tree: Record<string, TreeNodeSummary[]> = { root: [a], a: [b], b: [deep] }
const byId = new Map([root, a, b, deep].map((entry) => [entry.id, entry]))

function lens(staged: StageRequestItem[][] = []): LensPort {
  return {
    async summary() {
      return { state: 'ready', label: 'r', totalBytes: root.size, entryCount: 4, notes: [], root, topChildren: [a] }
    },
    async focus(nodeId) {
      const focus = byId.get(nodeId ?? 'root') ?? root
      return { node: focus, ancestors: [], children: tree[focus.id] ?? [], totalChildren: focus.childCount }
    },
    async children(nodeId, limit, offset): Promise<ChildrenPage> {
      const all = tree[nodeId] ?? []
      return { scanId, nodeId, items: all.slice(offset, offset + limit), offset, limit, total: all.length, sort: 'size' }
    },
    async cacheCandidates() {
      return { items: [], total: 0, totalSize: 0 }
    },
    async stage(items) {
      staged.push(items)
      return {
        staged: items.map((item, index) => ({
          id: `col_${index}`,
          scanId,
          nodeId: item.nodeId,
          path: byId.get(item.nodeId)?.path ?? item.nodeId,
          name: item.nodeId,
          size: 4_000,
          addedAt: '2026-10-22T00:00:00.000Z',
        })),
        rejected: [],
      }
    },
  }
}

function scripted(responses: ChatResponse[]): ChatPort {
  let index = 0
  return async () => responses[Math.min(index++, responses.length - 1)]!
}

const call = (name: string, args: unknown): ChatResponse => ({
  choices: [
    {
      message: {
        role: 'assistant',
        content: null,
        tool_calls: [{ id: `call_${name}`, type: 'function', function: { name, arguments: JSON.stringify(args) } }],
      },
    },
  ],
})
const final = (content: string): ChatResponse => ({ choices: [{ message: { role: 'assistant', content } }] })

test('a follow-up can stage an id the previous answer found; a fresh run cannot', async () => {
  const staged: StageRequestItem[][] = []
  const runtime = createToolRuntime()
  const first = await runAgent({
    chat: scripted([call('list_children', { nodeId: 'b' }), final('deep is 3.9 KB')]),
    lens: lens(staged),
    userMessage: 'what is inside b?',
    runtime,
  })
  assert.equal(first.stats.toolCalls, 1)

  const propose = call('propose_cleanup', { items: [{ nodeId: 'deep', reason: 'old build', risk: 'confirm' }] })
  const followUp = await runAgent({
    chat: scripted([propose, final('staged deep')]),
    lens: lens(staged),
    userMessage: 'stage it',
    runtime,
  })
  // Budgets restart per run; the seen set does not.
  assert.equal(followUp.stats.toolCalls, 1)
  assert.equal(followUp.staged.length, 1)
  assert.deepEqual(
    staged.at(-1)?.map((item) => item.nodeId),
    ['deep'],
  )

  const lonely = await runAgent({
    chat: scripted([propose, final('could not')]),
    lens: lens(),
    userMessage: 'stage it',
  })
  assert.equal(lonely.staged.length, 0)
  assert.match(lonely.messages.find((message) => message.role === 'tool')?.content ?? '', /unknown nodeId/)
})

function stagerHarness(initial: CollectorEntry[]) {
  let collector = initial
  const stager = createStager({
    scanId,
    resolve: async (id) => byId.get(id) ?? null,
    collector: () => collector,
    setCollector: (entries) => {
      collector = entries
    },
    newId: () => `col_${Math.random().toString(36).slice(2, 8)}`,
  })
  return { stager, entries: () => collector }
}

const entry = (nodeId: string, size: number, scan = scanId): CollectorEntry => ({
  id: `col_${nodeId}`,
  scanId: scan,
  nodeId,
  path: `/r/${nodeId}`,
  name: nodeId,
  size,
  addedAt: '2026-10-22T00:00:00.000Z',
})

test('list_proposal shows the live selection, the user’s picks included', async () => {
  const { stager } = stagerHarness([entry('mine', 2 * 1024 * 1024), entry('elsewhere', 1, 'scan_other0000')])
  await stager.stage([{ nodeId: 'deep', reason: 'old build output', risk: 'regenerable' }])
  const port: LensPort = { ...lens(), stage: stager.stage, proposal: stager.proposal, unstage: stager.unstage }
  const result = await executeTool(createToolRuntime(), port, 'list_proposal', '{}')
  assert.match(result.content, /^list_proposal rows=2 /)
  assert.match(result.content, /^mine\|mine\|2\|user\|$/m)
  assert.match(result.content, /^deep\|deep\|0\|regenerable\|old build output$/m)
  assert.doesNotMatch(result.content, /elsewhere/)
})

test('remove_from_proposal only shrinks the selection and reports misses', async () => {
  const { stager, entries } = stagerHarness([entry('keep_me', 10), entry('drop_me', 20)])
  const port: LensPort = { ...lens(), stage: stager.stage, proposal: stager.proposal, unstage: stager.unstage }
  const result = await executeTool(
    createToolRuntime(),
    port,
    'remove_from_proposal',
    JSON.stringify({ nodeIds: ['drop_me', 'never_staged'] }),
  )
  assert.match(result.content, /^remove_from_proposal removed=1 /)
  assert.match(result.content, /^\? never_staged\|not in the selection$/m)
  assert.deepEqual(
    entries().map((row) => row.nodeId),
    ['keep_me'],
  )
  const empty = await executeTool(createToolRuntime(), port, 'remove_from_proposal', '{"nodeIds":[]}')
  assert.match(empty.content, /^error remove_from_proposal/)
})

test('a scoped lens summarises the open folder and keeps cache rows inside it', async () => {
  const repo = node('repo', '/Users/me/Dev/repo', 900, 1)
  const service: LensService = {
    async scanStatus() {
      return {
        scanId,
        state: 'ready',
        message: '',
        progress: null,
        currentPath: null,
        bytesScanned: 10_000,
        entriesScanned: 99,
        rootIds: ['root'],
        label: 'me',
        updatedAt: '2026-10-22T00:00:00.000Z',
      }
    },
    async treeSlice({ nodeId }) {
      const focus = nodeId === 'repo' ? repo : root
      return {
        scanId,
        focusNode: focus,
        ancestors: [],
        tree: { ...focus, children: [] },
        truncated: false,
      } as unknown as Awaited<ReturnType<LensService['treeSlice']>>
    },
    async children() {
      throw new Error('unused')
    },
    async discover() {
      const item = (path: string, size: number): DiscoveryItem => ({
        node: node(path, path, size),
        parentId: null,
        category: 'cache',
        isDirectory: true,
      })
      return {
        scanId,
        kind: 'caches',
        items: [item('/Users/me/Dev/repo/node_modules', 500), item('/Users/me/Library/Caches/x', 4_000)],
        total: 2,
        totalSize: 4_500,
        offset: 0,
        limit: 200,
      } as unknown as Awaited<ReturnType<NonNullable<LensService['discover']>>>
    },
  }
  const scoped = lensPortFromService(service, scanId, lens().stage, {
    scope: { nodeId: 'repo', path: '/Users/me/Dev/repo' },
  })
  const summary = await scoped.summary()
  assert.equal(summary.root.id, 'repo')
  assert.equal(summary.scope, '/Users/me/Dev/repo')
  const caches = await scoped.cacheCandidates(20)
  assert.deepEqual(
    caches.items.map((item) => item.node.path),
    ['/Users/me/Dev/repo/node_modules'],
  )
  assert.equal(caches.totalSize, 500)

  const whole = lensPortFromService(service, scanId, lens().stage)
  assert.equal((await whole.summary()).scope, undefined)
  assert.equal((await whole.cacheCandidates(20)).items.length, 2)
})
