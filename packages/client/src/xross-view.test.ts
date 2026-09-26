import { describe, expect, it, vi } from 'vitest'
import { connectXrossView, readyRootNodeId, xrossHostFromWindow, XrossViewError } from './xross-view.js'
import { SPACE_LENS_VIEW_METHODS_V1 } from '../../../integrations/xross/view-contract/surfaces/space-lens/api.js'
import { parseSpaceLensId } from '../../../integrations/xross/view-contract/contracts/view-v1/ids.js'

const context = {
  contractMajor: 1, featureBits: ['space-lens.read'], packId: 'space-lens', packDigest: 'a'.repeat(64),
  xrossVersion: '1.0.0', sourceClientIncarnation: '1'.repeat(32),
  targetDeviceId: 'xdev_1aaaaaaaaaaaaaaaaaaaaaaaaa', targetDisplayLabel: 'Test Mac',
  targetPolicyRevision: '4', bridgeGeneration: '2'.repeat(32), locale: 'en',
}

function host() {
  const methods: Record<string, ReturnType<typeof vi.fn>> = Object.fromEntries(SPACE_LENS_VIEW_METHODS_V1.map((name) => [name, vi.fn()]))
  return Object.assign(methods, {
    context: vi.fn(async () => context),
    capabilities: vi.fn(async () => ({ apiMajor: 1, contractVersion: '1.0.0',
      scan: { start: true, cancel: true, maxConcurrent: 1 },
      cleanup: { plan: true, execute: true, mode: 'trash' }, providers: { iCloud: 'available' } })),
  })
}

describe('Xross Space Lens view boundary', () => {
  it('refuses an absent or incomplete host', async () => {
    expect(xrossHostFromWindow({})).toBeNull()
    await expect(connectXrossView(null)).rejects.toMatchObject({ code: 'HostUnavailable' })
    await expect(connectXrossView({ context: async () => context })).rejects.toMatchObject({ code: 'IncompatibleContract' })
  })

  it('captures the exact target generation and refuses a later target switch', async () => {
    const facade = host()
    const view = await connectXrossView(facade)
    facade.context.mockResolvedValueOnce({ ...context, bridgeGeneration: '3'.repeat(32) })
    await expect(view.listRoots()).rejects.toMatchObject({ code: 'StaleGeneration' })
    expect(facade['listRoots']).not.toHaveBeenCalled()
  })

  it('passes only opaque root ids into scan requests', async () => {
    const facade = host()
    facade['startScan']?.mockResolvedValue({ scanId: `xscan_${'0'.repeat(32)}`, state: 'queued', rootIds: [`xroot_${'0'.repeat(32)}`] })
    const view = await connectXrossView(facade)
    await view.startScan([`xroot_${'0'.repeat(32)}`])
    expect(facade['startScan']).toHaveBeenCalledWith({ rootIds: [`xroot_${'0'.repeat(32)}`], options: {
      ignoreHidden: false, respectGitignore: true, ignoredMode: 'summarize',
    } })
  })

  it('delegates cleanup and iCloud execution to native approval only', async () => {
    const facade = host()
    const view = await connectXrossView(facade)
    const planId = parseSpaceLensId('plan', `xplan_${'0'.repeat(32)}`)
    const iCloudPlanId = parseSpaceLensId('icloud', `xicloud_${'0'.repeat(32)}`)
    if (planId === null || iCloudPlanId === null) throw new Error('invalid fixture')
    await view.requestCleanupApproval(planId)
    await view.requestICloudEvictionApproval(iCloudPlanId)
    expect(facade['requestCleanupApproval']).toHaveBeenCalledTimes(1)
    expect(facade['requestICloudEvictionApproval']).toHaveBeenCalledTimes(1)
    expect(Reflect.has(facade, 'execute')).toBe(false)
  })

  it('reconciles a watch gap through getScan and never treats an event as a new scan request', async () => {
    const facade = host()
    const scanId = parseSpaceLensId('scan', `xscan_${'0'.repeat(32)}`)
    if (scanId === null) throw new Error('invalid fixture')
    const rootId = `xroot_${'0'.repeat(32)}`
    const ready = { scanId, state: 'ready', rootIds: [rootId], rootNodes: [{ rootId, nodeId: `xnode_${'0'.repeat(32)}` }],
      snapshotId: `xsnapshot_${'0'.repeat(32)}`, bytesVisited: '0', entriesVisited: '0', updatedAtUnixMs: '1' }
    facade['getScan']?.mockResolvedValue(ready)
    facade['watchScan']?.mockImplementation(async function* () {
      yield { kind: 'gap', sequence: '1' }
      yield { kind: 'terminal', sequence: '2' }
    })
    const view = await connectXrossView(facade)
    const events = []
    for await (const event of view.watchScan(scanId)) events.push(event)
    expect(events).toEqual([{ kind: 'reconciled', scan: ready }, { kind: 'scan', scan: ready, terminal: true }])
    expect(facade['startScan']).not.toHaveBeenCalled()
    expect(facade['getScan']).toHaveBeenCalledTimes(2)
  })

  it('uses bounded cursor and depth queries, with no path-based target operation', async () => {
    const facade = host()
    const view = await connectXrossView(facade)
    const snapshotId = parseSpaceLensId('snapshot', `xsnapshot_${'0'.repeat(32)}`)
    const nodeId = parseSpaceLensId('node', `xnode_${'0'.repeat(32)}`)
    if (snapshotId === null || nodeId === null) throw new Error('invalid fixture')
    await view.treeSlice(snapshotId, nodeId, 24)
    await view.childrenPage(snapshotId, nodeId, 'opaque-cursor' as never)
    expect(facade['treeSlice']).toHaveBeenCalledWith({ snapshotId, nodeId, query: { relativeDepth: 24, maxChildren: 50, maxNodes: 200 } })
    expect(facade['childrenPage']).toHaveBeenCalledWith({ snapshotId, nodeId, query: { limit: 200, cursor: 'opaque-cursor' } })
    expect(() => view.treeSlice(snapshotId, nodeId, 25)).toThrowError(XrossViewError)
  })

  it('refuses iCloud planning when the target reports unavailable', async () => {
    const facade = host()
    facade.capabilities.mockResolvedValue({ apiMajor: 1, contractVersion: '1.0.0',
      scan: { start: true, cancel: true, maxConcurrent: 1 }, cleanup: { plan: true, execute: true, mode: 'trash' },
      providers: { iCloud: 'unavailable' } })
    const view = await connectXrossView(facade)
    expect(() => view.planICloudEviction(`xroot_${'0'.repeat(32)}` as never, `xsnapshot_${'0'.repeat(32)}` as never))
      .toThrowError(XrossViewError)
    expect(facade['planICloudEviction']).not.toHaveBeenCalled()
  })

  it('composes paged root-to-parent breadcrumbs with opaque cursors', async () => {
    const facade = host()
    const nodeId = parseSpaceLensId('node', `xnode_${'1'.repeat(32)}`)
    const snapshotId = parseSpaceLensId('snapshot', `xsnapshot_${'1'.repeat(32)}`)
    if (nodeId === null || snapshotId === null) throw new Error('invalid fixture')
    facade['listAncestors']?.mockResolvedValueOnce({ items: [{ nodeId: 'first' }], nextCursor: 'cur_next' })
      .mockResolvedValueOnce({ items: [{ nodeId: 'second' }], nextCursor: null })
    const view = await connectXrossView(facade)
    expect(await view.allAncestors(snapshotId, nodeId)).toEqual([{ nodeId: 'first' }, { nodeId: 'second' }])
    expect(facade['listAncestors']).toHaveBeenNthCalledWith(2, { snapshotId, nodeId, query: { limit: 200, cursor: 'cur_next' } })
  })

  it('refuses a scan when target capability denies start without invoking the host', async () => {
    const facade = host()
    facade.capabilities.mockResolvedValue({ apiMajor: 1, contractVersion: '1.0.0',
      scan: { start: false, cancel: false, maxConcurrent: 0 }, cleanup: { plan: false, execute: false, mode: 'none' },
      providers: { iCloud: 'unavailable' } })
    const view = await connectXrossView(facade)
    expect(() => view.startScan([`xroot_${'1'.repeat(32)}`])).toThrowError(XrossViewError)
    expect(facade['startScan']).not.toHaveBeenCalled()
  })

  it('selects the matching ready root node even when bindings arrive in reverse order', () => {
    const first = `xroot_${'1'.repeat(32)}`
    const second = `xroot_${'2'.repeat(32)}`
    const firstNode = `xnode_${'1'.repeat(32)}`
    const secondNode = `xnode_${'2'.repeat(32)}`
    const ready = { scanId: `xscan_${'1'.repeat(32)}`, state: 'ready',
      rootIds: [first, second], rootNodes: [{ rootId: second, nodeId: secondNode }, { rootId: first, nodeId: firstNode }],
      snapshotId: `xsnapshot_${'1'.repeat(32)}`, bytesVisited: '0', entriesVisited: '0', updatedAtUnixMs: '1' }
    expect(readyRootNodeId(ready as never, first as never)).toBe(firstNode)
    expect(readyRootNodeId(ready as never, second as never)).toBe(secondNode)
    expect(() => readyRootNodeId({ ...ready, rootNodes: [{ rootId: first, nodeId: firstNode }] } as never, first as never)).toThrowError(XrossViewError)
    expect(() => readyRootNodeId({ ...ready, rootNodes: [{ rootId: first, nodeId: firstNode }, { rootId: second, nodeId: firstNode }] } as never, first as never)).toThrowError(XrossViewError)
  })
})
