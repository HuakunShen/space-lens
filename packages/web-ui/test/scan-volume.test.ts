import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { ScanTarget, ScanVolume } from '../src/types.ts'
import { resolveScanVolume, scanVolumeLabel, volumeCapacity } from '../src/lib/scan-volume.ts'

const volume = (path: string, totalBytes = 1000, freeBytes = 250): ScanVolume => ({
  path,
  totalBytes,
  freeBytes,
  availableBytes: 200,
  isLocal: true,
})

test('host volumes fill missing scan metadata and choose the deepest containing mount', () => {
  const root = volume('/')
  const external = volume('/Volumes/Portable2TB/')
  assert.equal(resolveScanVolume('/Users/hk/project', [], [root, external]), root)
  assert.equal(resolveScanVolume('/Volumes/Portable2TB/project/', [root], [external]), external)
  assert.equal(resolveScanVolume('/Volumes/Portable2TB', [], [external]), external)
  assert.equal(resolveScanVolume('/Volumes/Portable2TB-old', [], [root, external]), root)
})

test('host mount identity and current capacity win over query-path scan metadata', () => {
  const scanned = volume('/Volumes/Drive', 1000, 100)
  const host = volume('/Volumes/Drive/', 1000, 300)
  assert.equal(resolveScanVolume('/Volumes/Drive/project', [scanned], [host]), host)
  const folder = volume('/Volumes/Drive/project', 1000, 100)
  assert.equal(resolveScanVolume('/Volumes/Drive/project/subfolder', [folder], [host]), host)
  assert.equal(resolveScanVolume('/Volumes/Drive/project', [volume('/Volumes/Drive', 0)], [host]), host)
  assert.equal(resolveScanVolume('/Volumes/Drive/project', [volume('/Volumes/Drive', NaN)], [host]), host)
})

test('query paths do not masquerade as nested mounts', () => {
  const unknown = volume('/Volumes/Drive', 0)
  const host = volume('/')
  assert.equal(resolveScanVolume('/Volumes/Drive/project', [unknown], [host]), host)
})

test('scan-only capacity uses an honest label when its actual mount is unknown', () => {
  const queried = volume('/Users/hk/project')
  const resolved = resolveScanVolume('/Users/hk/project/src', [queried], [])
  assert.ok(resolved)
  assert.deepEqual(volumeCapacity(resolved), { total: 1000, used: 750, free: 250, usedPercent: 75 })
  assert.equal(scanVolumeLabel(resolved, []), 'Disk containing this folder')
})

test('scan capacity can fill an unknown host capacity while keeping the verified mount identity', () => {
  const host = volume('/Volumes/Drive', 0)
  const resolved = resolveScanVolume('/Volumes/Drive/project', [volume('/Volumes/Drive/project')], [host])
  assert.ok(resolved)
  assert.equal(resolved.path, '/Volumes/Drive')
  assert.equal(scanVolumeLabel(resolved, []), 'Drive')
  assert.deepEqual(volumeCapacity(resolved), { total: 1000, used: 750, free: 250, usedPercent: 75 })
  assert.equal(resolveScanVolume('/Volumes/Drive/project', [volume('/')], [volume('/'), host]), host)
})

test('missing, relative, or unrelated paths never select an arbitrary disk', () => {
  const volumes = [volume('/Volumes/Drive')]
  for (const path of [null, undefined, '', 'Scan label', '/Users/hk', '/Volumes/Drive-old']) {
    assert.equal(resolveScanVolume(path, [], volumes), null)
  }
  assert.equal(resolveScanVolume('project', [], [volume('/')]), null)
})

test('Windows drive and UNC paths match separators and case without matching another drive or share', () => {
  const drive = volume('C:\\')
  const share = volume('\\\\server\\share\\')
  assert.equal(resolveScanVolume('c:/Users/hk', [], [drive]), drive)
  assert.equal(resolveScanVolume('D:\\Users\\hk', [], [drive]), null)
  assert.equal(resolveScanVolume('//SERVER/share/project', [], [share]), share)
  assert.equal(resolveScanVolume('\\\\server\\share-other', [], [share]), null)
  assert.equal(resolveScanVolume('\\\\server\\share-other', [], [volume('/')]), null)
})

test('capacity preserves zero free space and distinguishes unknown totals from an empty disk', () => {
  assert.deepEqual(volumeCapacity(volume('/', 1000, 0)), { total: 1000, used: 1000, free: 0, usedPercent: 100 })
  assert.deepEqual(volumeCapacity(volume('/', 1000, 1000)), { total: 1000, used: 0, free: 1000, usedPercent: 0 })
  assert.equal(volumeCapacity(volume('/', 0, 0)), null)
  assert.equal(volumeCapacity(volume('/', NaN)), null)
  assert.equal(volumeCapacity(null), null)
})

test('capacity falls back to valid available bytes when free is invalid, and rejects impossible counts', () => {
  assert.deepEqual(volumeCapacity(volume('/', 1000, NaN)), { total: 1000, used: 800, free: 200, usedPercent: 80 })
  assert.equal(volumeCapacity({ ...volume('/'), freeBytes: 1001, availableBytes: 1001 }), null)
  assert.equal(volumeCapacity({ ...volume('/'), freeBytes: -1, availableBytes: -1 }), null)
})

test('drive labels use matching volume targets, never the scanned folder name', () => {
  const target: ScanTarget = {
    id: 'root',
    path: '/',
    label: 'Macintosh HD',
    kind: 'volume',
    description: '',
    source: 'root',
    size: 0,
    removable: false,
  }
  assert.equal(scanVolumeLabel(volume('/'), [target]), 'Macintosh HD')
  assert.equal(scanVolumeLabel(volume('/'), [{ ...target, kind: 'folder', label: 'Project' }]), 'Filesystem')
  assert.equal(scanVolumeLabel(volume('/Volumes/Portable2TB/'), []), 'Portable2TB')
  assert.equal(scanVolumeLabel(volume('C:\\'), []), 'C:')
})
