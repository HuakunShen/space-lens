// Unit tests for the memory view's segment-bar geometry and colour ramp (T17).
import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { ProcessGroup } from '@space-lens/contract'
import { layoutSegments, segmentColor } from '../src/lib/memory/segments.ts'

function group(groupId: string, name: string, footprintBytes: number): ProcessGroup {
  return {
    groupId,
    name,
    bundleId: null,
    bundlePath: null,
    kind: 'app',
    mainPid: Number(groupId.slice(-4)),
    pids: [Number(groupId.slice(-4))],
    cpuPercent: null,
    footprintBytes,
    residentBytes: footprintBytes,
    frontmost: false,
    lastFrontmostAt: null,
    startedAt: '2026-10-18T00:00:00.000Z',
    protection: null,
  }
}

test('segments below the minimum width fold into a single Other', () => {
  const groups = [group('grp_000000000000000a', 'A', 500), group('grp_000000000000000b', 'B', 400), group('grp_000000000000000c', 'C', 10), group('grp_000000000000000d', 'D', 90)]
  const segments = layoutSegments(groups, 1000, 100)
  assert.deepEqual(segments.map((s) => s.label), ['A', 'B', 'D', 'Other (1)'])
  const other = segments[segments.length - 1]!
  assert.equal(other.groupId, null)
  assert.equal(other.bytes, 10)
  assert.equal(segments[0]!.share, 0.5)
  assert.equal(segments[0]!.rank, 0)
  assert.equal(other.rank, 3)
})

test('a segment exactly at the minimum width is kept', () => {
  const groups = [group('grp_000000000000000a', 'A', 500), group('grp_000000000000000b', 'B', 400), group('grp_000000000000000c', 'C', 100)]
  const segments = layoutSegments(groups, 1000, 20, 24, 2)
  assert.deepEqual(segments.map((s) => s.label), ['A', 'B', 'C'])
  assert.equal(segments[2]!.bytes, 100)
})

test('the bar keeps at most max groups and folds the tail into Other', () => {
  const groups = Array.from({ length: 30 }, (_, i) => group(`grp_${i.toString(16).padStart(16, '0')}`, `G${i}`, 100))
  const segments = layoutSegments(groups, 3000, 1000)
  assert.equal(segments.length, 25)
  assert.equal(segments[24]!.label, 'Other (6)')
  assert.equal(segments[24]!.bytes, 600)
  assert.equal(segments[24]!.rank, 24)
})

test('empty or degenerate inputs produce no segments', () => {
  assert.deepEqual(layoutSegments([], 1000, 100), [])
  assert.deepEqual(layoutSegments([group('grp_000000000000000a', 'A', 100)], 0, 100), [])
  assert.deepEqual(layoutSegments([group('grp_000000000000000a', 'A', 100)], 1000, 0), [])
})

test('segmentColor walks pink to purple to teal by rank', () => {
  const hues = (count: number) =>
    Array.from({ length: count }, (_, rank) => {
      const match = /oklch\(([\d.]+)% ([\d.]+) ([\d.]+)\)/.exec(segmentColor(rank, count))
      assert.ok(match, 'segmentColor returns an oklch() string')
      return Number(match![3])
    })
  const five = hues(5)
  for (let i = 1; i < five.length; i += 1) assert.ok(five[i]! < five[i - 1]!, 'hue decreases with rank')
  assert.ok(five[0]! > 320 && five[0]! < 350, 'rank 0 is pink')
  assert.ok(five[five.length - 1]! < 200, 'last rank is teal')
  assert.equal(segmentColor(0, 1), segmentColor(0, 4), 'a single segment is the pink end')
  assert.match(segmentColor(2, 9), /^oklch\([\d.]+% [\d.]+ [\d.]+\)$/)
})
