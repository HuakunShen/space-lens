// Unit tests for software presets: rule matching, memory totals that count a
// nested program once, scoping of the views, and storage totals.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { AppAuditReport, MemorySnapshot, ProcessRow } from '@space-lens/contract'
import {
  AI_PRESET,
  ruleMatchesProcess,
  scopeSnapshot,
  summarizePresetMemory,
  summarizePresetStorage,
  type SoftwarePreset,
} from '../src/lib/presets/index.ts'

function row(
  pid: number,
  name: string,
  ppid: number | null,
  footprintBytes: number,
  command: string | null = null,
): ProcessRow {
  return {
    pid,
    ppid,
    name,
    startTimeUs: pid * 1_000_000,
    groupId: `grp_${pid.toString(16).padStart(16, '0')}`,
    ownedByCurrentUser: true,
    kind: 'app',
    cpuPercent: null,
    footprintBytes,
    residentBytes: footprintBytes,
    virtualBytes: null,
    restricted: false,
    protection: null,
    command,
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
      totalBytes: 1000,
      usedBytes: 1000,
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

// ChatGPT (20 B) → codex (5 B) — nested; Claude helper (40 B) — separate tree.
const processes = [
  row(1, 'launchd', null, 1),
  row(10, 'ChatGPT', 1, 20, '/Applications/ChatGPT.app/Contents/MacOS/ChatGPT'),
  row(11, 'codex', 10, 5, 'node /usr/local/lib/node_modules/@openai/codex/bin/codex.js'),
  row(
    20,
    'Claude Helper',
    1,
    40,
    '/Applications/Claude.app/Contents/Frameworks/Claude Helper.app/Contents/MacOS/Claude Helper',
  ),
  row(30, 'Safari', 1, 300, '/Applications/Safari.app/Contents/MacOS/Safari'),
]

test('a rule matches by name case-insensitively or by command pattern', () => {
  const rule = AI_PRESET.rules.find((candidate) => candidate.id === 'codex')!
  assert.equal(ruleMatchesProcess(rule, { name: 'CODEX', command: null }), true)
  assert.equal(ruleMatchesProcess(rule, { name: 'node', command: 'node /x/@openai/codex/bin/codex.js' }), true)
  assert.equal(ruleMatchesProcess(rule, { name: 'node', command: 'node server.js' }), false)
})

test('memory totals count a nested program once and report per-rule subtrees', () => {
  const summary = summarizePresetMemory(snapshot(processes), AI_PRESET)
  // ChatGPT subtree (25) + Claude helper (40); codex is inside ChatGPT's subtree.
  assert.equal(summary.totalBytes, 65)
  assert.equal(summary.processCount, 3)

  const byId = new Map(summary.rows.map((r) => [r.rule.id, r]))
  assert.equal(byId.get('chatgpt')!.bytes, 25)
  assert.equal(byId.get('chatgpt')!.processCount, 2)
  assert.equal(byId.get('codex')!.bytes, 5, 'codex still reports its own subtree')
  assert.equal(byId.get('claude-desktop')!.bytes, 40)
  assert.equal(byId.get('cursor')!.bytes, 0)
})

test('scoping keeps one program with its children, or every matched program', () => {
  const one = scopeSnapshot(snapshot(processes), AI_PRESET, 'codex')
  assert.deepEqual(
    one.processes.map((p) => p.pid),
    [11],
  )

  const chatgpt = scopeSnapshot(snapshot(processes), AI_PRESET, 'chatgpt')
  assert.deepEqual(
    chatgpt.processes.map((p) => p.pid),
    [10, 11],
  )

  const all = scopeSnapshot(snapshot(processes), AI_PRESET, null)
  assert.deepEqual(
    all.processes.map((p) => p.pid),
    [10, 11, 20],
  )
})

test('scoped groups total only the kept processes', () => {
  const withGroups: MemorySnapshot = {
    ...snapshot(processes),
    groups: [
      {
        groupId: 'grp_000000000000000a',
        name: 'ChatGPT',
        bundleId: null,
        bundlePath: null,
        kind: 'app',
        mainPid: 10,
        pids: [10, 11],
        cpuPercent: null,
        footprintBytes: 25,
        residentBytes: 25,
        frontmost: false,
        lastFrontmostAt: null,
      } as unknown as MemorySnapshot['groups'][number],
    ],
  }
  const scoped = scopeSnapshot(withGroups, AI_PRESET, 'codex')
  // The group's footprint sums only kept members of that group (pid 10 here).
  assert.deepEqual(scoped.groups[0]!.pids, [11], 'only the kept pid stays in the group')
  assert.equal(scoped.groups[0]!.footprintBytes, 0, 'pid 11 belongs to another group')
  assert.equal(
    scopeSnapshot(withGroups, AI_PRESET, 'cursor').groups.length,
    0,
    'a group with no kept process is dropped',
  )
  const chatgpt = scopeSnapshot(withGroups, AI_PRESET, 'chatgpt')
  assert.deepEqual(chatgpt.groups[0]!.pids, [10, 11])
  assert.equal(chatgpt.groups[0]!.footprintBytes, 20)
})

test('a preset with no matching processes totals to zero', () => {
  const preset: SoftwarePreset = {
    id: 'none',
    label: 'None',
    description: '',
    rules: [{ id: 'x', name: 'X', storage: [], match: { names: ['nope'] } }],
  }
  const summary = summarizePresetMemory(snapshot(processes), preset)
  assert.equal(summary.totalBytes, 0)
  assert.equal(summary.processCount, 0)
})

test('storage totals sum each catalog entry once per rule', () => {
  const report = {
    apps: [
      { id: 'claude-desktop', totalSize: 100 },
      { id: 'claude-code-cli', totalSize: 30 },
      { id: 'codex-cli', totalSize: 50 },
      { id: 'caveman', totalSize: 7 },
      { id: 'unrelated', totalSize: 999 },
    ],
  } as unknown as AppAuditReport
  const summary = summarizePresetStorage(report, AI_PRESET)
  const byId = new Map(summary.rows.map((r) => [r.rule.id, r.bytes]))
  assert.equal(byId.get('claude-desktop'), 100)
  assert.equal(byId.get('claude-code'), 30)
  assert.equal(byId.get('codex'), 50)
  assert.equal(byId.get('caveman'), 7)
  assert.equal(summary.totalBytes, 187)
  assert.equal(summarizePresetStorage(null, AI_PRESET).totalBytes, 0)
})
