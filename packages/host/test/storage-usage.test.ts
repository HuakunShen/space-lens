import { describe, expect, it } from 'vitest'

import { readStorageUsage, storageUsageArgs } from '../src/storage-usage.ts'

const BREAKDOWN = {
  container: 'disk3',
  capacityBytes: 1000,
  inUseBytes: 520,
  purgeableBytes: 119,
  scannedBytes: 287,
  volumes: [{ name: 'VM', role: 'VM', device: 'disk3s6', inUseBytes: 30 }],
  snapshots: { volume: 'disk3s5', count: 25, timeMachineCount: 25, oldestTimeMachine: '2026-10-05T16:01:11' },
}

describe('readStorageUsage', () => {
  const roots = [
    { path: '/', bytes: 287 },
    { path: '/Volumes/My Disk', bytes: 5 },
  ]

  it('pairs every root with its scanned bytes, in order', () => {
    expect(storageUsageArgs(roots)).toEqual([
      'storage-usage',
      '--root',
      '/',
      '--scanned-bytes',
      '287',
      '--root',
      '/Volumes/My Disk',
      '--scanned-bytes',
      '5',
    ])
  })

  it('returns the validated breakdown from the CLI', async () => {
    const calls: string[][] = []
    const found = await readStorageUsage(
      '/bin/spacelens',
      roots,
      async (_binary, args) => {
        calls.push([...args])
        return JSON.stringify(BREAKDOWN)
      },
      'darwin',
    )
    expect(found).toEqual(BREAKDOWN)
    expect(calls).toHaveLength(1)
  })

  it('degrades to no breakdown instead of failing the scan', async () => {
    const answer = (stdout: string | null) => async () => stdout
    expect(await readStorageUsage('/bin/x', roots, answer('null'), 'darwin')).toBeUndefined()
    expect(await readStorageUsage('/bin/x', roots, answer('not json'), 'darwin')).toBeUndefined()
    expect(await readStorageUsage('/bin/x', roots, answer(null), 'darwin')).toBeUndefined()
    expect(
      await readStorageUsage('/bin/x', roots, answer(JSON.stringify({ ...BREAKDOWN, extra: 1 })), 'darwin'),
    ).toBeUndefined()
  })

  it('never spawns off macOS or without roots', async () => {
    const run = async () => {
      throw new Error('must not spawn')
    }
    expect(await readStorageUsage('/bin/x', roots, run, 'linux')).toBeUndefined()
    expect(await readStorageUsage('/bin/x', [], run, 'darwin')).toBeUndefined()
  })
})
