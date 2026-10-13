import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  appCleanupAvailable,
  revealAvailable,
} from '../src/lib/gates.ts'
import { shouldApplyAuditReport } from '../src/lib/app-storage.ts'
import {
  isPathCovered,
  pruneStaged,
  validateStagedAgainstReport,
} from '../../../packages/web-ui/src/lib/app-audit.ts'
import type { AppAuditReport, Capabilities } from '@space-lens/contract'

const capabilities = (
  cleanup: Partial<Capabilities['cleanup']> = {},
  host: Partial<Capabilities['host']> = {},
): Capabilities => ({
  apiMajor: 1,
  contractVersion: '1.0.0',
  scan: { start: true, cancel: true, discovery: true, appAudit: true, maxConcurrent: 1 },
  cleanup: { plan: true, execute: true, mode: 'trash', appPlan: true, appExecute: true, ...cleanup },
  host: { folderPicker: false, appLocationReveal: true, ...host },
  icloud: 'unavailable',
})

const fullService = {
  appCleanupPlan: () => Promise.reject(new Error('unused')),
  appCleanupExecute: () => Promise.reject(new Error('unused')),
  revealAppLocation: () => Promise.reject(new Error('unused')),
}

test('app cleanup needs both submit arms declared and implemented', () => {
  assert.equal(appCleanupAvailable({ capabilities: capabilities(), service: fullService }), true)
  assert.equal(
    appCleanupAvailable({ capabilities: capabilities({ appPlan: false }), service: fullService }),
    false,
  )
  assert.equal(
    appCleanupAvailable({ capabilities: capabilities({ appExecute: undefined }), service: fullService }),
    false,
  )
  assert.equal(
    appCleanupAvailable({ capabilities: capabilities(), service: { appCleanupPlan: fullService.appCleanupPlan } }),
    false,
  )
  assert.equal(appCleanupAvailable({ capabilities: capabilities(), service: {} }), false)
  assert.equal(appCleanupAvailable({ capabilities: null, service: fullService }), false)
  assert.equal(appCleanupAvailable({ capabilities: capabilities(), service: null }), false)
})

test('reveal needs the host flag and the reveal method', () => {
  assert.equal(revealAvailable({ capabilities: capabilities(), service: fullService }), true)
  assert.equal(
    revealAvailable({ capabilities: capabilities({}, { appLocationReveal: false }), service: fullService }),
    false,
  )
  assert.equal(
    revealAvailable({ capabilities: capabilities({}, { appLocationReveal: undefined }), service: fullService }),
    false,
  )
  assert.equal(revealAvailable({ capabilities: capabilities(), service: {} }), false)
  assert.equal(revealAvailable({ capabilities: null, service: fullService }), false)
  assert.equal(revealAvailable({ capabilities: capabilities(), service: null }), false)
})

test('late audit replies never swap the report', () => {
  assert.equal(shouldApplyAuditReport({ request: 3, sequence: 3, view: 'apps' }), true)
  // A reply from before a newer refresh started is stale.
  assert.equal(shouldApplyAuditReport({ request: 2, sequence: 3, view: 'apps' }), false)
  // Leaving the view drops the reply even when the generation matches.
  assert.equal(shouldApplyAuditReport({ request: 3, sequence: 3, view: 'browse' }), false)
})

test('staging covers descendants of a staged parent folder', () => {
  const staged = new Set(['/Users/hk/Library/Application Support/com.example'])
  assert.equal(isPathCovered('/Users/hk/Library/Application Support/com.example/node', staged), true)
  assert.equal(isPathCovered('/Users/hk/Library/Application Support/com.example', staged), false)
  assert.equal(isPathCovered('/Users/hk/Library/Caches/com.example', staged), false)
})

test('refresh pruning keeps only paths the new report still lists', () => {
  assert.deepEqual(pruneStaged(['/a', '/b'], ['/b', '/c']), ['/b'])
  const report: AppAuditReport = {
    platform: 'macos',
    generatedAt: new Date('2026-10-13T00:00:00.000Z').toISOString(),
    apps: [
      {
        id: 'raycast',
        name: 'Raycast',
        kind: 'app',
        source: 'catalog',
        bundleId: 'com.raycast.macos',
        appPath: '/Applications/Raycast.app',
        installedSize: 0,
        totalSize: 10,
        locations: [
          {
            path: '/keep',
            label: null,
            classification: 'cache',
            purgeable: true,
            comment: null,
            size: 10,
            source: 'heuristic',
          },
        ],
      },
    ],
    totalApps: 1,
    truncated: false,
  }
  const validated = validateStagedAgainstReport(
    new Map([['raycast', new Set(['/keep', '/drop'])]]),
    report,
  )
  assert.deepEqual([...(validated.get('raycast') ?? [])], ['/keep'])
})
