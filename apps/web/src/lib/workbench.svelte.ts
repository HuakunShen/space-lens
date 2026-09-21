import type { CollectorEntry, ScanStatus, ScanTarget, TreeNodeSummary, TreeSlice } from '@space-lens/contract'
import type { HttpService } from '@space-lens/client'
import { connectEventStream } from '@space-lens/client'

export const BASE_URL_KEY = 'spacelens.baseUrl'
export const TOKEN_KEY = 'spacelens.session.token'
export const RECENT_KEY = 'spacelens.recentScans'

/** Backend address resolution is pure runtime: ?api= → remembered → same origin. */
export function resolveBaseUrl(explicit?: string | null): { url: string; sameOrigin: boolean } {
  const fromQuery = explicit ?? new URLSearchParams(window.location.search).get('api') ?? undefined
  const remembered = window.localStorage.getItem(BASE_URL_KEY) ?? undefined
  const raw = fromQuery ?? remembered ?? window.location.origin
  const url = raw.replace(/\/$/, '')
  const sameOrigin = url === window.location.origin
  return { url, sameOrigin }
}

export function rememberBaseUrl(url: string): void {
  window.localStorage.setItem(BASE_URL_KEY, url.replace(/\/$/, ''))
}

/** The pairing ticket may arrive as ?pair=, #pair=, ?ticket= or #ticket=. */
export function ticketFromLocation(): string | null {
  const url = new URL(window.location.href)
  const from = (key: string): string | null =>
    url.searchParams.get(key) ?? (url.hash.includes(`${key}=`) ? new URLSearchParams(url.hash.slice(1)).get(key) : null)
  return from('pair') ?? from('ticket')
}

export function stripTicketFromLocation(): void {
  const url = new URL(window.location.href)
  let changed = false
  for (const key of ['pair', 'ticket']) {
    if (url.searchParams.has(key)) {
      url.searchParams.delete(key)
      changed = true
    }
  }
  if (url.hash.includes('pair=') || url.hash.includes('ticket=')) {
    url.hash = ''
    changed = true
  }
  if (changed) window.history.replaceState(null, '', url.toString())
}

export function loadToken(): string | null {
  return window.sessionStorage.getItem(TOKEN_KEY)
}

export function saveToken(token: string): void {
  window.sessionStorage.setItem(TOKEN_KEY, token)
}

export function clearToken(): void {
  window.sessionStorage.removeItem(TOKEN_KEY)
}

export interface RecentTarget {
  path: string
  label: string
  lastScannedAt: string
}

export function loadRecentTargets(): RecentTarget[] {
  try {
    return JSON.parse(window.localStorage.getItem(RECENT_KEY) ?? '[]') as RecentTarget[]
  } catch {
    return []
  }
}

export function rememberRecentTarget(path: string): void {
  const label = path.split('/').pop() || path
  const next = [
    { path, label, lastScannedAt: new Date().toISOString() },
    ...loadRecentTargets().filter((entry) => entry.path !== path),
  ].slice(0, 5)
  window.localStorage.setItem(RECENT_KEY, JSON.stringify(next))
}

export interface WorkbenchState {
  phase: 'idle' | 'connecting' | 'ready' | 'failed'
  connectMessage: string | null
  resolvedUrl: string | null
  sameOrigin: boolean
  service: HttpService | null
  capabilities: Awaited<ReturnType<HttpService['capabilities']>> | null
  targets: ScanTarget[]
  status: ScanStatus | null
  activeScanId: string | null
  slice: TreeSlice | null
  items: TreeNodeSummary[]
  hoveredId: string | null
  collector: CollectorEntry[]
  deleting: boolean
  error: string | null
  streamState: 'connecting' | 'live' | 'reconnecting' | 'closed' | 'idle'
  closingScanIds: string[]
}

export const workbench = $state<WorkbenchState>({
  phase: 'idle',
  connectMessage: null,
  resolvedUrl: null,
  sameOrigin: true,
  service: null,
  capabilities: null,
  targets: [],
  status: null,
  activeScanId: null,
  slice: null,
  items: [],
  hoveredId: null,
  collector: [],
  deleting: false,
  error: null,
  streamState: 'idle',
  closingScanIds: [],
})

export type StreamHandle = { close: () => void }

export function startEventStream(): StreamHandle | undefined {
  const service = workbench.service
  if (service === null) return undefined
  workbench.streamState = 'connecting'
  return connectEventStream({
    baseUrl: service.baseUrl,
    getToken: () => loadToken(),
    since: 0,
    onState: (state) => {
      workbench.streamState = state
    },
    onGap: (from, to) => {
      // a gap means we missed events; the poll fallback below reconciles
      void from
      void to
    },
    onEvent: (envelope) => {
      if (envelope.payload.kind === 'scan.updated' || envelope.payload.kind === 'scan.completed') {
        if (envelope.payload.status.scanId !== workbench.activeScanId) return
        workbench.status = envelope.payload.status
        if (envelope.payload.status.state === 'ready') {
          void loadRoot()
        }
      }
    },
  })
}

export async function loadRoot(): Promise<void> {
  const { service, status } = workbench
  if (service === null || status === null || status.state !== 'ready' || status.rootIds.length === 0) return
  const rootId = status.rootIds[0]
  const slice = await service.treeSlice({ scanId: status.scanId, nodeId: rootId, depth: 3, maxChildrenPerNode: 50 })
  workbench.slice = slice
  const page = await service.children({ scanId: status.scanId, nodeId: rootId, offset: 0, limit: 200, sort: 'size' })
  workbench.items = page.items
}

export function describeError(error: unknown): string {
  if (error instanceof Error) return error.message
  return String(error)
}
