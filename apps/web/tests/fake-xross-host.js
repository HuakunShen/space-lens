// @ts-nocheck — a browser-injected fixture, not application code.
/** Browser-only fake host for the Xross entry; never included in a pack. */
(() => {
  const suffix = (digit) => digit.repeat(32)
  const rootId = `xroot_${suffix('1')}`
  const rootNodeId = `xnode_${suffix('1')}`
  const childNodeId = `xnode_${suffix('2')}`
  const scanId = `xscan_${suffix('1')}`
  const snapshotId = `xsnapshot_${suffix('1')}`
  const jobId = `xjob_${suffix('1')}`
  const root = { nodeId: rootNodeId, name: 'Macintosh HD', displayPath: '/Users/private',
    sizeBytes: '2048', depth: 0, childCount: 1, ignored: false, collapsed: false, hasChildren: true }
  const child = { nodeId: childNodeId, parentId: rootNodeId, name: 'Documents', displayPath: '/Users/private/Documents',
    sizeBytes: '1024', depth: 1, childCount: 0, ignored: false, collapsed: false, hasChildren: false }
  const scan = { scanId, state: 'ready', bytesVisited: '2048', entriesVisited: '2', rootIds: [rootId],
    rootNodes: [{ rootId, nodeId: rootNodeId }], snapshotId, updatedAtUnixMs: '1735689600000' }
  const job = { jobId, state: 'running', sequence: '1', completedCount: 0, totalCount: 1,
    completedBytes: '0', totalBytes: '1024', outcomes: [] }
  const calls = []
  window.__xrossFakeCalls = calls
  const called = (name, value, request) => {
    calls.push({ name, request })
    document.documentElement.dataset.xrossFakeCalls = JSON.stringify(calls)
    return value
  }
  window.xrossSpaceLensV1 = {
    context: async () => ({ contractMajor: 1, featureBits: ['space-lens.read'], packId: 'space-lens',
      packDigest: 'a'.repeat(64), xrossVersion: '1.0.0', sourceClientIncarnation: '1'.repeat(32),
      targetDeviceId: 'xdev_1aaaaaaaaaaaaaaaaaaaaaaaaa', targetDisplayLabel: 'Test Mac',
      targetPolicyRevision: '1', bridgeGeneration: '2'.repeat(32),
      locale: new URLSearchParams(location.search).get('locale') ?? 'en' }),
    capabilities: async () => ({ apiMajor: 1, contractVersion: '1.0.0',
      scan: { start: true, cancel: true, maxConcurrent: 1 }, cleanup: { plan: true, execute: true, mode: 'trash' },
      providers: { iCloud: location.search.includes('nonmac') ? 'unavailable' : 'available' } }),
    listRoots: async () => called('listRoots', { items: [{ rootId, label: 'Macintosh HD', kind: 'volume',
      providerFeatures: ['filesystem', 'icloud', 'trash'] }], nextCursor: null }),
    startScan: async (request) => called('startScan', scan, request),
    getScan: async () => called('getScan', scan),
    async *watchScan() { yield { kind: 'terminal', sequence: '1' } },
    cancelScan: async () => called('cancelScan', { ...scan, state: 'cancelled' }),
    treeSlice: async ({ nodeId }) => called('treeSlice', { kind: 'treeSlice', snapshotId, focusNodeId: nodeId,
      tree: nodeId === rootNodeId ? [root, child] : [child], ancestors: nodeId === rootNodeId ? [] : [root],
      totalBytes: nodeId === rootNodeId ? '2048' : '1024', truncated: false, omittedBytes: '0', omittedCount: 0,
      generatedAtUnixMs: '1735689600000' }, { nodeId }),
    childrenPage: async ({ nodeId }) => called('childrenPage', { kind: 'childrenPage', snapshotId, nodeId,
      items: nodeId === rootNodeId ? [child] : [], nextCursor: null, total: nodeId === rootNodeId ? 1 : 0, sort: 'size' }),
    listAncestors: async ({ nodeId }) => called('listAncestors', { kind: 'ancestorsPage', snapshotId, nodeId,
      items: nodeId === rootNodeId ? [] : [root], nextCursor: null }),
    listCandidates: async () => called('listCandidates', { items: [], nextCursor: null }),
    previewCleanup: async ({ nodeIds }) => called('previewCleanup', { planId: `xplan_${suffix('1')}`, snapshotId,
      selected: nodeIds.map((nodeId) => ({ nodeId, displayPath: '/Users/private/Documents', sizeBytes: '1024', reasonCode: 'node' })),
      totalBytes: '1024', mode: 'trash', createdAtUnixMs: String(Date.now()), expiresAtUnixMs: String(Date.now() + 600000) }, { nodeIds }),
    requestCleanupApproval: async (request) => called('requestCleanupApproval', { decision: 'approved', outcome: job }, request),
    planICloudEviction: async () => called('planICloudEviction', { planId: `xicloud_${suffix('1')}`, rootId, snapshotId,
      selectedCount: 1, bytesToEvict: '1024', expiresAtUnixMs: String(Date.now() + 600000) }),
    requestICloudEvictionApproval: async (request) => called('requestICloudEvictionApproval', { decision: 'approved', outcome: job }, request),
    getCleanupJob: async () => called('getCleanupJob', job),
    async *watchCleanup() { yield { kind: 'terminal', sequence: '2' } },
    controlICloudEviction: async ({ command }) => called('controlICloudEviction', { ...job,
      state: command === 'pause' ? 'paused' : command === 'resume' ? 'running' : 'cancelled' }),
  }
})()
