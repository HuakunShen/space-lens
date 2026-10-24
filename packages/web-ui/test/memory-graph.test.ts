// Unit tests for the Memory view's focus graph: ancestry, sibling context,
// node budget, layout coordinates and edge kinds.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { MemorySnapshot, ProcessRow } from '@space-lens/contract'
import { buildForest } from '../src/lib/memory/tree.ts'
import { GRAPH_X_GAP, GRAPH_Y_GAP, buildProcessGraph } from '../src/lib/memory/graph.ts'

function row(
  pid: number,
  name: string,
  ppid: number | null,
  footprintBytes: number | null = null,
): ProcessRow {
  return {
    pid,
    ppid,
    name,
    startTimeUs: pid * 1_000_000,
    groupId: `grp_${pid.toString(16).padStart(16, '0')}`,
    ownedByCurrentUser: true,
    kind: 'cli',
    cpuPercent: null,
    footprintBytes,
    residentBytes: footprintBytes,
    virtualBytes: null,
    restricted: false,
    protection: null,
    command: null,
    user: null,
    state: 'sleeping',
    threads: null,
    listeningPorts: [],
  }
}

function snapshot(processes: ProcessRow[]): MemorySnapshot {
  return {
    sequence: 1,
    sampledAt: '2026-10-18T00:00:00.000Z',
    intervalMs: 1000,
    platform: 'macos',
    breakdown: {
      totalBytes: 100,
      usedBytes: 100,
      freeBytes: 0,
      availableBytes: null,
      appBytes: null,
      wiredBytes: null,
      compressedBytes: null,
      cachedFilesBytes: null,
      swapTotalBytes: 0,
      swapUsedBytes: 0,
    },
    pressure: { level: 'normal', source: 'kernel', availablePercent: null },
    groups: [],
    processes,
    notes: [],
  }
}

/** launchd → login → zsh → cargo → rustc → {cc, ld}; clippy is rustc's sibling. */
function rustcForest() {
  return buildForest(
    snapshot([
      row(1, 'launchd', null, 4000),
      row(2, 'login', 1, 900),
      row(3, 'zsh', 2, 800),
      row(5, 'cargo', 3, 700),
      row(10, 'clippy', 5, 100),
      row(6, 'rustc', 5, 1500),
      row(7, 'cc', 6, 600),
      row(8, 'ld', 6, 500),
      row(4, 'vim', 3, 300),
    ]),
  )
}

test('an unknown pid has no graph', () => {
  assert.equal(buildProcessGraph(rustcForest(), 999), null)
})

test('the chain above, the focus and the subtree below become one diagram', () => {
  const graph = buildProcessGraph(rustcForest(), 6)!
  const node = (pid: number) => graph.nodes.find((candidate) => candidate.pid === pid)!

  assert.equal(graph.focusPid, 6)
  assert.equal(graph.focus.pid, 6)
  assert.equal(graph.focus.role, 'focus')
  assert.equal(node(1).role, 'ancestor')
  assert.equal(node(5).role, 'ancestor')
  assert.equal(node(10).role, 'sibling')
  assert.equal(node(7).role, 'descendant')
  assert.equal(node(8).role, 'descendant')
  // vim is a sibling of cargo, not of the focus, so it stays out.
  assert.equal(graph.nodes.some((candidate) => candidate.pid === 4), false)

  assert.deepEqual(
    graph.edges.map((edge) => `${edge.source}>${edge.target}:${edge.kind}`).sort(),
    ['1>2:chain', '2>3:chain', '3>5:chain', '5>10:context', '5>6:chain', '6>7:branch', '6>8:branch'],
  )
  assert.equal(graph.truncated, false)
  assert.equal(graph.hiddenProcesses, 0)
  assert.equal(graph.focusDescendantCount, 2)
})

test('the focused node sits at x = 0 with its parents above and children below', () => {
  const graph = buildProcessGraph(rustcForest(), 6)!
  const node = (pid: number) => graph.nodes.find((candidate) => candidate.pid === pid)!

  assert.equal(node(6).x, 0)
  assert.equal(node(5).y, node(6).y - GRAPH_Y_GAP)
  assert.equal(node(1).y, node(6).y - 4 * GRAPH_Y_GAP)
  assert.equal(node(7).y, node(6).y + GRAPH_Y_GAP)
  // cc and ld take two leaf slots around the focus; clippy takes the next one.
  assert.equal(node(7).x, -GRAPH_X_GAP / 2)
  assert.equal(node(8).x, GRAPH_X_GAP / 2)
  assert.equal(node(10).x, GRAPH_X_GAP * 1.5)
  // The ancestry is a straight spine above the focus, whatever the siblings do.
  assert.equal(node(5).x, node(6).x)
  assert.equal(node(3).x, node(6).x)
  assert.equal(node(1).x, node(6).x)
})

test('sibling context is capped and the left-out children are counted', () => {
  const processes = [row(1, 'launchd', null, 5000)]
  for (let index = 0; index < 8; index += 1) {
    processes.push(row(100 + index, `worker${index}`, 1, 900 - index * 10))
  }
  processes.push(row(2, 'focus', 1, 400))
  processes.push(row(3, 'child', 2, 100))
  const graph = buildProcessGraph(buildForest(snapshot(processes)), 2, { maxSiblings: 3 })!

  const siblings = graph.nodes.filter((candidate) => candidate.role === 'sibling')
  assert.equal(siblings.length, 3)
  // The forest sorts children by memory, so the biggest three come along.
  assert.deepEqual(siblings.map((candidate) => candidate.name), ['worker0', 'worker1', 'worker2'])
  const root = graph.nodes.find((candidate) => candidate.pid === 1)!
  assert.equal(root.hiddenChildren, 5)
  assert.equal(graph.truncated, true)
})

test('siblings can be switched off', () => {
  const graph = buildProcessGraph(rustcForest(), 6, { maxSiblings: 0 })!
  assert.equal(graph.nodes.some((candidate) => candidate.role === 'sibling'), false)
  const cargo = graph.nodes.find((candidate) => candidate.pid === 5)!
  assert.equal(cargo.hiddenChildren, 1)
})

test('a wide fan-out keeps a tree shape: children per parent and per level are capped', () => {
  const processes = [row(1, 'launchd', null, 9000), row(2, 'java', 1, 8000)]
  for (let index = 0; index < 30; index += 1) {
    processes.push(row(100 + index, `helper${index}`, 2, 1000 - index))
  }
  // Every helper launches two children of its own, so a second generation exists.
  for (let index = 0; index < 30; index += 1) {
    processes.push(row(200 + index * 2, `worker${index}a`, 100 + index, 50))
    processes.push(row(201 + index * 2, `worker${index}b`, 100 + index, 40))
  }
  const graph = buildProcessGraph(buildForest(snapshot(processes)), 2)!

  const firstLevel = graph.nodes.filter((candidate) => candidate.depth === graph.focus.depth + 1)
  const secondLevel = graph.nodes.filter((candidate) => candidate.depth === graph.focus.depth + 2)
  assert.equal(firstLevel.length, 6, 'six children per parent')
  assert.equal(secondLevel.length, 12, 'twelve nodes per generation')
  assert.deepEqual(
    firstLevel.map((candidate) => candidate.name),
    ['helper0', 'helper1', 'helper2', 'helper3', 'helper4', 'helper5'],
  )
  assert.equal(graph.focus.hiddenChildren, 24)
  assert.equal(graph.hiddenProcesses, 30 + 60 - 18)
})

test('the node cap folds the rest of a big branch instead of cutting the chain', () => {
  const processes = [row(1, 'launchd', null, 9000), row(2, 'java', 1, 8000)]
  for (let index = 0; index < 20; index += 1) {
    processes.push(row(100 + index, `thread${index}`, 2, 500 - index))
  }
  const graph = buildProcessGraph(buildForest(snapshot(processes)), 2, {
    maxNodes: 6,
    maxSiblings: 0,
  })!

  assert.equal(graph.nodes.length, 6)
  assert.equal(graph.nodes.filter((candidate) => candidate.role === 'ancestor').length, 1)
  assert.equal(graph.focus.role, 'focus')
  assert.equal(graph.focus.hiddenChildren, 16)
  assert.equal(graph.hiddenProcesses, 16)
  assert.equal(graph.focusDescendantCount, 20)
  assert.equal(graph.truncated, true)
})

test('a long chain keeps its end so the focus is never cut', () => {
  const processes: ProcessRow[] = []
  for (let index = 0; index < 10; index += 1) {
    processes.push(row(index + 1, `p${index}`, index === 0 ? null : index, 100))
  }
  const graph = buildProcessGraph(buildForest(snapshot(processes)), 10, { maxAncestors: 3 })!

  assert.equal(graph.hiddenAncestors, 7)
  assert.deepEqual(
    graph.nodes.filter((candidate) => candidate.role !== 'descendant').map((candidate) => candidate.pid),
    [8, 9, 10],
  )
  assert.equal(graph.focus.role, 'focus')
  assert.equal(graph.focus.pid, 10)
})

test('the focus keeps a single position when its own subtree grows', () => {
  const small = buildProcessGraph(rustcForest(), 6)!
  const withChild = buildProcessGraph(rustcForest(), 7)!
  assert.equal(small.focus.x, 0)
  assert.equal(withChild.focus.x, 0)
})
