/** A standalone, typed Xross view boundary; never a path-authoritative WorkbenchService. */
import { parseSpaceLensId, type SpaceICloudPlanIdV1, type SpaceJobIdV1, type SpaceNodeIdV1, type SpacePlanIdV1, type SpaceRootIdV1, type SpaceScanIdV1, type SpaceSnapshotIdV1 } from '../../../integrations/xross/view-contract/contracts/view-v1/ids.js'
import type { CursorV1 } from '../../../integrations/xross/view-contract/contracts/view-v1/types.js'
import { SPACE_LENS_VIEW_METHODS_V1, type ScanStatusV1, type SpaceLensViewApiV1 } from '../../../integrations/xross/view-contract/surfaces/space-lens/api.js'
import { isCurrentViewContext, parseViewContextV1, type ViewContextV1 } from '../../../integrations/xross/view-contract/surfaces/view-context.js'

export class XrossViewError extends Error {
  constructor(readonly code: 'HostUnavailable' | 'IncompatibleContract' | 'StaleGeneration' | 'PermissionDenied' | 'UnsupportedOnTarget' | 'MalformedPayload') {
    super(code)
  }
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isHost(value: unknown): value is SpaceLensViewApiV1 {
  return record(value) && SPACE_LENS_VIEW_METHODS_V1.every((method) => typeof value[method] === 'function')
}

export function xrossHostFromWindow(scope: object): SpaceLensViewApiV1 | null {
  const value = Reflect.get(scope, 'xrossSpaceLensV1')
  return isHost(value) ? value : null
}

function rootId(value: string): SpaceRootIdV1 {
  const parsed = parseSpaceLensId('root', value)
  if (parsed === null) throw new XrossViewError('MalformedPayload')
  return parsed
}

function validatedScan(scan: ScanStatusV1): ScanStatusV1 {
  if (!record(scan) || parseSpaceLensId('scan', scan.scanId) === null ||
      !['queued', 'scanning', 'ready', 'cancelled', 'failed'].includes(scan.state) ||
      !Array.isArray(scan.rootIds) || scan.rootIds.length < 1 || scan.rootIds.length > 16) {
    throw new XrossViewError('MalformedPayload')
  }
  const roots = new Set(scan.rootIds)
  if (roots.size !== scan.rootIds.length || scan.rootIds.some((id) => parseSpaceLensId('root', id) === null)) {
    throw new XrossViewError('MalformedPayload')
  }
  if (scan.state !== 'ready') {
    if (scan.snapshotId !== undefined || scan.rootNodes !== undefined) throw new XrossViewError('MalformedPayload')
    return scan
  }
  if (scan.snapshotId === undefined || parseSpaceLensId('snapshot', scan.snapshotId) === null ||
      !Array.isArray(scan.rootNodes) ||
      scan.rootNodes.length !== scan.rootIds.length) throw new XrossViewError('MalformedPayload')
  const boundRoots = new Set<string>()
  const boundNodes = new Set<string>()
  for (const binding of scan.rootNodes) {
    if (!roots.has(binding.rootId) || boundRoots.has(binding.rootId) || boundNodes.has(binding.nodeId) ||
        parseSpaceLensId('node', binding.nodeId) === null) throw new XrossViewError('MalformedPayload')
    boundRoots.add(binding.rootId)
    boundNodes.add(binding.nodeId)
  }
  return scan
}

export function readyRootNodeId(scan: ScanStatusV1, selectedRootId: SpaceRootIdV1): SpaceNodeIdV1 {
  validatedScan(scan)
  const nodeId = scan.rootNodes?.find((binding) => binding.rootId === selectedRootId)?.nodeId
  if (nodeId === undefined) throw new XrossViewError('MalformedPayload')
  return nodeId
}

export async function connectXrossView(input: unknown) {
  if (input === null || input === undefined) throw new XrossViewError('HostUnavailable')
  if (!isHost(input)) throw new XrossViewError('IncompatibleContract')
  const host = input
  const context = parseViewContextV1(await host.context())
  if (context === null || context.packId !== 'space-lens' || context.contractMajor !== 1) {
    throw new XrossViewError('IncompatibleContract')
  }
  const sessionContext: ViewContextV1 = context
  const capabilities = await host.capabilities()
  if (capabilities.apiMajor !== 1 || !capabilities.scan || !capabilities.cleanup || !capabilities.providers) {
    throw new XrossViewError('IncompatibleContract')
  }
  async function current(): Promise<void> {
    const fresh = parseViewContextV1(await host.context())
    if (fresh === null || !isCurrentViewContext(sessionContext, fresh)) throw new XrossViewError('StaleGeneration')
  }
  async function checked<T>(call: () => Promise<T>): Promise<T> {
    await current()
    const result = await call()
    await current()
    return result
  }
  await current()
  return {
    context: sessionContext,
    capabilities,
    current,
    listRoots: (cursor?: CursorV1) => checked(() => host.listRoots({ query: { limit: 100, ...(cursor ? { cursor } : {}) } })),
    startScan: (ids: readonly string[]) => {
      if (!capabilities.scan.start) throw new XrossViewError('PermissionDenied')
      if (ids.length === 0 || ids.length > 16) throw new XrossViewError('MalformedPayload')
      return checked(() => host.startScan({ rootIds: ids.map(rootId), options: {
        ignoreHidden: false, respectGitignore: true, ignoredMode: 'summarize',
      } })).then(validatedScan)
    },
    getScan: (scanId: SpaceScanIdV1) => checked(() => host.getScan({ scanId })).then(validatedScan),
    cancelScan: (scanId: SpaceScanIdV1) => {
      if (!capabilities.scan.cancel) throw new XrossViewError('PermissionDenied')
      return checked(() => host.cancelScan({ scanId })).then(validatedScan)
    },
    async *watchScan(scanId: SpaceScanIdV1) {
      await current()
      let lastSequence: string | undefined
      for await (const event of host.watchScan({ scanId })) {
        await current()
        if (event.kind === 'gap') {
          yield { kind: 'reconciled' as const, scan: validatedScan(await checked(() => host.getScan({ scanId }))) }
        } else if (event.kind === 'event' || event.kind === 'terminal') {
          if (lastSequence !== undefined && BigInt(event.sequence) <= BigInt(lastSequence)) continue
          lastSequence = event.sequence
          if (event.kind === 'terminal') {
            yield { kind: 'scan' as const, scan: validatedScan(await checked(() => host.getScan({ scanId }))), terminal: true }
            break
          }
          yield { kind: 'scan' as const, scan: validatedScan(event.value.scan), terminal: false }
        }
      }
    },
    treeSlice: (snapshotId: SpaceSnapshotIdV1, nodeId: SpaceNodeIdV1, relativeDepth = 3) => {
      if (!Number.isInteger(relativeDepth) || relativeDepth < 0 || relativeDepth > 24) throw new XrossViewError('MalformedPayload')
      return checked(() => host.treeSlice({ snapshotId, nodeId, query: { relativeDepth, maxChildren: 50, maxNodes: 200 } }))
    },
    childrenPage: (snapshotId: SpaceSnapshotIdV1, nodeId: SpaceNodeIdV1, cursor?: CursorV1) =>
      checked(() => host.childrenPage({ snapshotId, nodeId, query: { limit: 200, ...(cursor ? { cursor } : {}) } })),
    listAncestors: (snapshotId: SpaceSnapshotIdV1, nodeId: SpaceNodeIdV1, cursor?: CursorV1) =>
      checked(() => host.listAncestors({ snapshotId, nodeId, query: { limit: 200, ...(cursor ? { cursor } : {}) } })),
    async allAncestors(snapshotId: SpaceSnapshotIdV1, nodeId: SpaceNodeIdV1) {
      const items: Awaited<ReturnType<typeof host.listAncestors>>['items'][number][] = []
      let cursor: CursorV1 | null = null
      for (let pageIndex = 0; pageIndex < 8; pageIndex += 1) {
        const page = await checked(() => host.listAncestors({ snapshotId, nodeId, query: { limit: 200, ...(cursor ? { cursor } : {}) } }))
        items.push(...page.items)
        if (page.nextCursor === null) return items
        if (page.nextCursor === cursor) throw new XrossViewError('MalformedPayload')
        cursor = page.nextCursor
      }
      throw new XrossViewError('MalformedPayload')
    },
    listCandidates: (snapshotId: SpaceSnapshotIdV1, cursor?: CursorV1) =>
      checked(() => host.listCandidates({ snapshotId, query: { limit: 200, ...(cursor ? { cursor } : {}) } })),
    previewCleanup: (snapshotId: SpaceSnapshotIdV1, nodeIds: readonly SpaceNodeIdV1[]) => {
      if (!capabilities.cleanup.plan) throw new XrossViewError('PermissionDenied')
      return checked(() => host.previewCleanup({ snapshotId, nodeIds }))
    },
    requestCleanupApproval: (planId: SpacePlanIdV1) => {
      if (!capabilities.cleanup.execute) throw new XrossViewError('PermissionDenied')
      return checked(() => host.requestCleanupApproval({ planId }))
    },
    planICloudEviction: (selectedRootId: SpaceRootIdV1, snapshotId: SpaceSnapshotIdV1) => {
      if (capabilities.providers.iCloud !== 'available') throw new XrossViewError('UnsupportedOnTarget')
      return checked(() => host.planICloudEviction({ rootId: selectedRootId, snapshotId }))
    },
    requestICloudEvictionApproval: (planId: SpaceICloudPlanIdV1) => {
      if (capabilities.providers.iCloud !== 'available') throw new XrossViewError('UnsupportedOnTarget')
      return checked(() => host.requestICloudEvictionApproval({ planId }))
    },
    getCleanupJob: (jobId: SpaceJobIdV1) => checked(() => host.getCleanupJob({ jobId })),
    async *watchCleanup(jobId: SpaceJobIdV1) {
      await current()
      for await (const event of host.watchCleanup({ jobId })) {
        await current()
        yield event
      }
    },
    controlICloudEviction: (jobId: SpaceJobIdV1, command: 'pause' | 'resume' | 'cancel') =>
      checked(() => host.controlICloudEviction({ jobId, command })),
  }
}

export type XrossView = Awaited<ReturnType<typeof connectXrossView>>
export type XrossScanStatus = ScanStatusV1
