import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { AppAuditReport, TreeNodeSummary } from '@space-lens/contract'
import {
  annotationForNode,
  assertRealPath,
  canCollectNode,
  collectRefusalReason,
  eligibilityOf,
  isAccountingLeaf,
  isAncestorById,
  isAppBundleNode,
  isAppScanStatus,
  isOmittedBucket,
  isVirtualAppId,
  isVirtualAppPath,
  parentIdMap,
} from '../src/lib/app-scan.ts'

const node = (overrides: Partial<TreeNodeSummary> = {}): TreeNodeSummary => ({
  id: 'node_1',
  name: 'folder',
  path: '/tmp/folder',
  size: 100,
  depth: 1,
  ignored: false,
  collapsed: false,
  hasChildren: false,
  childCount: 0,
  scanState: 'complete',
  ...overrides,
})

test('virtual identifiers, bundle leaves and omitted buckets are recognized', () => {
  assert.equal(isVirtualAppPath('app-storage:abcdef1234567890abcdef'), true)
  assert.equal(isVirtualAppPath('/tmp/real'), false)
  assert.equal(isVirtualAppPath(null), false)
  assert.equal(isVirtualAppId('approot_abcdef1234567890abcdef'), true)
  assert.equal(isVirtualAppId('approot:raycast'), true)
  assert.equal(isVirtualAppId('node_1'), false)
  assert.equal(isAppBundleNode(node({ id: 'appbundle_abcdef1234567890abcdef' })), true)
  assert.equal(isAppBundleNode(node({ id: 'appbundle:raycast' })), true)
  assert.equal(isAppBundleNode(node()), false)
  assert.equal(isOmittedBucket(node({ id: 'parent:omitted' })), true)
  assert.equal(isOmittedBucket(node()), false)
})

test('accounting leaves are detected by id suffix or host flag', () => {
  assert.equal(isAccountingLeaf(node({ id: 'parent:accounting' })), true)
  assert.equal(isAccountingLeaf(node({ id: 'parent:own' })), true)
  assert.equal(isAccountingLeaf({ ...node(), accounting: true } as TreeNodeSummary), true)
  assert.equal(isAccountingLeaf(node()), false)
  assert.equal(isAccountingLeaf(node({ id: 'parent:omitted' })), false)
})

test('eligibility refuses structure first, then consults the host, then falls back', () => {
  assert.equal(eligibilityOf(node({ path: 'app-storage:abc', id: 'approot_abc' })), 'refused')
  assert.equal(eligibilityOf(node({ id: 'appbundle_abc' })), 'refused')
  assert.equal(eligibilityOf(node({ id: 'x:accounting' })), 'refused')
  assert.equal(eligibilityOf(node({ id: 'x:omitted' })), 'refused')
  assert.equal(eligibilityOf(node({ scanState: 'skipped' })), 'refused')
  assert.equal(eligibilityOf(node({ scanState: 'partial' })), 'refused')
  assert.equal(eligibilityOf(node()), 'cleanable')
  // Host predicate wins over the scanState fallback.
  assert.equal(eligibilityOf({ ...node({ scanState: 'partial' }), eligibility: 'cleanable' } as TreeNodeSummary), 'cleanable')
  assert.equal(
    eligibilityOf({ ...node(), eligibility: 'contents-only' } as TreeNodeSummary),
    'contents-only',
  )
  assert.equal(eligibilityOf({ ...node(), eligibility: 'refused' } as TreeNodeSummary), 'refused')
  // Unknown host values are ignored, not trusted.
  assert.equal(eligibilityOf({ ...node(), eligibility: 'yes-please' } as TreeNodeSummary), 'cleanable')
  assert.equal(canCollectNode(node()), true)
  assert.equal(canCollectNode(node({ scanState: 'partial' })), false)
  assert.equal(canCollectNode({ ...node(), eligibility: 'contents-only' } as TreeNodeSummary), false)
})

test('refusal reasons explain the disabled control', () => {
  assert.match(collectRefusalReason(node({ path: 'app-storage:abc' })) ?? '', /virtual root/)
  assert.match(collectRefusalReason(node({ id: 'appbundle_abc' })) ?? '', /uninstall/)
  assert.match(collectRefusalReason(node({ id: 'x:accounting' })) ?? '', /own files/)
  assert.match(collectRefusalReason(node({ id: 'x:omitted' })) ?? '', /Grouped items/)
  assert.match(
    collectRefusalReason({ ...node(), eligibility: 'contents-only' } as TreeNodeSummary) ?? '',
    /contents instead/,
  )
  assert.equal(collectRefusalReason(node()), null)
})

test('ancestry checks follow id parent chains, never path prefixes', () => {
  const parents = new Map([
    ['loc', 'approot_abc'],
    ['child', 'loc'],
    ['grandchild', 'child'],
    ['other', 'approot_abc'],
  ])
  assert.equal(isAncestorById('approot_abc', 'grandchild', parents), true)
  assert.equal(isAncestorById('loc', 'grandchild', parents), true)
  assert.equal(isAncestorById('grandchild', 'loc', parents), false)
  assert.equal(isAncestorById('loc', 'loc', parents), true)
  assert.equal(isAncestorById('other', 'grandchild', parents), false)
  assert.equal(isAncestorById('loc', 'grandchild', null), false)
  const tree = {
    id: 'approot_abc',
    children: [
      { id: 'loc', children: [{ id: 'child', children: [] }] },
      { id: 'other', children: [] },
    ],
  }
  const map = parentIdMap(tree)
  assert.equal(map.get('loc'), 'approot_abc')
  assert.equal(map.get('child'), 'loc')
  assert.equal(map.get('other'), 'approot_abc')
  assert.equal(map.has('approot_abc'), false)
  assert.equal(parentIdMap(null).size, 0)
})

test('inspector annotations join depth-1 location nodes to the pinned report', () => {
  const report: AppAuditReport = {
    platform: 'macos',
    generatedAt: '2026-10-13T00:00:00.000Z',
    auditId: 'audit_abcdef1234567890abcdef',
    apps: [
      {
        id: 'raycast',
        name: 'Raycast',
        kind: 'app',
        source: 'catalog',
        bundleId: null,
        appPath: '/Applications/Raycast.app',
        installedSize: 10,
        totalSize: 110,
        locations: [
          {
            path: '/Users/demo/Library/Caches/com.raycast',
            label: 'Cache',
            classification: 'cache',
            purgeable: true,
            comment: 'Safe to clear',
            size: 100,
            source: 'catalog',
          },
        ],
      },
    ],
    totalApps: 1,
    truncated: false,
  }
  const location = node({ path: '/Users/demo/Library/Caches/com.raycast' })
  const annotation = annotationForNode(location, true, report, 'raycast')
  assert.deepEqual(annotation, {
    label: 'Cache',
    classification: 'cache',
    comment: 'Safe to clear',
    purgeable: true,
    source: 'catalog',
  })
  // Not a depth-1 child of the virtual root: no annotation.
  assert.equal(annotationForNode(location, false, report, 'raycast'), null)
  // Bundle leaves and unknown paths never annotate.
  assert.equal(annotationForNode(node({ id: 'appbundle_abc', path: '/Applications/Raycast.app' }), true, report, 'raycast'), null)
  assert.equal(annotationForNode(node({ path: '/elsewhere' }), true, report, 'raycast'), null)
  assert.equal(annotationForNode(location, true, report, 'other-app'), null)
  assert.equal(annotationForNode(location, true, null, 'raycast'), null)
})

test('app scan status is derived from the origin marker and virtual paths never reach the host', () => {
  assert.equal(isAppScanStatus({ app: { appId: 'a', appName: 'A', auditId: 'audit_abcdef1234567890abcdef' } }), true)
  assert.equal(isAppScanStatus({}), false)
  assert.equal(isAppScanStatus(null), false)
  assert.throws(() => assertRealPath('app-storage:abc', 'reveal'), /synthetic/)
  assert.doesNotThrow(() => assertRealPath('/tmp/real', 'reveal'))
})
