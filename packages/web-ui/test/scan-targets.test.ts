import { test } from 'node:test'
import assert from 'node:assert/strict'
import { scanTargetPaths } from '../src/lib/scan-targets.ts'
import type { ScanTarget } from '@space-lens/contract'
const target = (path: string, source: 'root' | 'recent' = 'root'): ScanTarget => ({ id: path, path, label: path, kind: 'volume', description: '', source, size: 0, removable: false })
test('startup disk includes Data only when explicitly offered by the current host', () => {
  const system = target('/')
  assert.deepEqual(scanTargetPaths(system, [system]), ['/'])
  assert.deepEqual(scanTargetPaths(system, [system, target('/System/Volumes/Data', 'recent')]), ['/'])
  assert.deepEqual(scanTargetPaths(system, [system, target('/System/Volumes/Data')]), ['/', '/System/Volumes/Data'])
  assert.deepEqual(scanTargetPaths(target('/Volumes/Portable2TB'), [system, target('/System/Volumes/Data')]), ['/Volumes/Portable2TB'])
})
