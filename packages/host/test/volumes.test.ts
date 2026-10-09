import { describe, expect, it } from 'vitest'

import { mountedVolumes, volumeCapacities } from '../src/volumes.ts'

const DF_PATHS = `Filesystem 1024-blocks Used Available Capacity Mounted on
/dev/disk3s1 1000000 300000 700000 30% /
/dev/disk9s1 2000000 800000 1200000 40% /Volumes/Portable2TB
`

const DF_ALL = `Filesystem 1024-blocks Used Available Capacity Mounted on
/dev/disk3s1 1000000 300000 700000 30% /
devfs 215 215 0 100% /dev
/dev/disk3s6 1000000 10000 700000 2% /System/Volumes/VM
map auto_home 0 0 0 100% /System/Volumes/Data/home
/dev/disk9s1 2000000 800000 1200000 40% /Volumes/Portable2TB
`

describe('volumeCapacities', () => {
  it('maps each path to its filesystem totals', async () => {
    const found = await volumeCapacities(['/', '/Volumes/Portable2TB'], async () => DF_PATHS)
    expect(found.get('/')).toEqual({ totalBytes: 1000000 * 1024, usedBytes: 300000 * 1024 })
    expect(found.get('/Volumes/Portable2TB')).toEqual({ totalBytes: 2000000 * 1024, usedBytes: 800000 * 1024 })
  })

  it('shares one line across paths on the same filesystem', async () => {
    // `df` collapses two paths on one filesystem into a single line; the
    // mount column assigns both.
    const same = `Filesystem 1024-blocks Used Available Capacity Mounted on
/dev/disk3s1 1000000 300000 700000 30% /
`
    const found = await volumeCapacities(['/', '/Users/hk'], async () => same)
    expect(found.get('/Users/hk')).toEqual(found.get('/'))
  })

  it('omits what df cannot report instead of fabricating zeros', async () => {
    expect(await volumeCapacities([], async () => DF_PATHS)).toEqual(new Map())
    expect(await volumeCapacities(['/missing'], async () => null)).toEqual(new Map())
    expect(await volumeCapacities(['/x'], async () => 'garbage\n')).toEqual(new Map())
  })
})

describe('mountedVolumes', () => {
  it('keeps real disks and drops virtual and system-internal mounts', async () => {
    const volumes = await mountedVolumes(async () => DF_ALL)
    expect(volumes.map((v) => v.path)).toEqual(['/', '/Volumes/Portable2TB'])
    const root = volumes[0]!
    expect(root.totalBytes).toBe(1000000 * 1024)
    expect(root.freeBytes).toBe(root.availableBytes)
    expect(root.isLocal).toBe(true)
  })

  it('returns nothing when df fails', async () => {
    expect(await mountedVolumes(async () => null)).toEqual([])
  })
})
