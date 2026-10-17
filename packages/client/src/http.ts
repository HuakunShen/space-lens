import type {
  AppAuditReport,
  AppAuditRequest,
  AppIcon,
  AppIconRequest,
  AiChatRequest,
  AiConfigView,
  Capabilities,
  ChildrenPage,
  ChildrenPageRequest,
  CleanupExecuteRequest,
  CleanupOutcome,
  CleanupPlan,
  CleanupPlanRequest,
  DiscoveryRequest,
  DiscoveryPage,
  Health,
  Problem,
  RootsResponse,
  VolumesResponse,
  ScanListResponse,
  ScanSession,
  ScanStartRequest,
  ScanStatus,
  Session,
  TreeSlice,
  TreeSliceRequest,
} from '@space-lens/contract'
import { ScanListResponseSchema } from '@space-lens/contract'

/** One client-side failure kind, shaped like the wire Problem. */
export class ServiceError extends Error {
  readonly code: Problem['code']
  readonly status: number
  readonly retryable: boolean

  constructor(problem: Problem, status: number) {
    super(problem.message)
    this.name = 'ServiceError'
    this.code = problem.code
    this.status = status
    this.retryable = problem.retryable
  }
}

export type ConnectionState = 'connecting' | 'ready' | 'reconnecting' | 'disconnected' | 'failed'

export interface HttpServiceOptions {
  baseUrl: string
  /** Returns the current bearer token, if a session exists. */
  getToken: () => string | null
  fetchImpl?: typeof fetch
}

async function raiseProblem(response: Response): Promise<never> {
  let problem: Problem = { code: 'InternalError', message: `request failed (${response.status})`, retryable: false }
  try {
    const body = (await response.json()) as { problem?: Problem }
    if (body.problem && typeof body.problem.code === 'string') problem = body.problem
  } catch {
    // non-JSON error body: keep the fallback problem
  }
  throw new ServiceError(problem, response.status)
}

/**
 * Thin authenticated REST client over the closed contract. The token never
 * leaves this closure except through explicit getters the page controls.
 */
export function createHttpService(options: HttpServiceOptions) {
  const doFetch = options.fetchImpl ?? fetch
  const base = options.baseUrl.replace(/\/$/, '')

  /** Shared by every method: JSON content type plus the current bearer token. */
  const authHeaders = (extra?: HeadersInit): Record<string, string> => {
    const token = options.getToken()
    const headers: Record<string, string> = {
      'content-type': 'application/json',
      ...(extra as Record<string, string> | undefined),
    }
    if (token !== null) headers.authorization = `Bearer ${token}`
    return headers
  }

  const request = async <T>(path: string, init?: RequestInit): Promise<T> => {
    const response = await doFetch(`${base}${path}`, { ...init, headers: authHeaders(init?.headers) })
    if (!response.ok) await raiseProblem(response)
    if (response.status === 204) return undefined as T
    return (await response.json()) as T
  }

  const scanList = async (): Promise<ScanListResponse> =>
    ScanListResponseSchema.parse(await request<unknown>('/api/v1/scans'))

  return {
    kind: 'http' as const,
    baseUrl: base,
    health: () => request<Health>('/health'),
    exchange: (ticket: string, password?: string) =>
      request<Session>('/api/v1/session/exchange', {
        method: 'POST',
        body: JSON.stringify({ ticket, ...(password ? { password } : {}) }),
      }),
    capabilities: () => request<Capabilities>('/api/v1/capabilities'),
    roots: () => request<RootsResponse>('/api/v1/roots'),
    volumes: () => request<VolumesResponse>('/api/v1/volumes'),
    startScan: (body: ScanStartRequest) =>
      request<ScanSession>('/api/v1/scans', { method: 'POST', body: JSON.stringify(body) }),
    scanList,
    listScans: async () => (await scanList()).scans,
    scanStatus: (scanId: string) => request<ScanStatus>(`/api/v1/scans/${scanId}`),
    cancelScan: (scanId: string) =>
      request<ScanStatus>(`/api/v1/scans/${scanId}/cancel`, { method: 'POST', body: '{}' }),
    treeSlice: (body: TreeSliceRequest) =>
      request<TreeSlice>('/api/v1/tree/slice', { method: 'POST', body: JSON.stringify(body) }),
    children: (body: ChildrenPageRequest) =>
      request<ChildrenPage>('/api/v1/tree/children', { method: 'POST', body: JSON.stringify(body) }),
    discover: (body: DiscoveryRequest) =>
      request<DiscoveryPage>('/api/v1/discovery', { method: 'POST', body: JSON.stringify(body) }),
    appAudit: (body: AppAuditRequest) =>
      request<AppAuditReport>('/api/v1/apps/audit', { method: 'POST', body: JSON.stringify(body) }),
    appIcon: (body: AppIconRequest) =>
      request<AppIcon>('/api/v1/apps/icon', { method: 'POST', body: JSON.stringify(body) }),
    plan: (body: CleanupPlanRequest) =>
      request<CleanupPlan>('/api/v1/cleanup/plan', { method: 'POST', body: JSON.stringify(body) }),
    execute: (body: CleanupExecuteRequest) =>
      request<CleanupOutcome>('/api/v1/cleanup/execute', { method: 'POST', body: JSON.stringify(body) }),
    /** Operator-owned AI advisor config. Only the host can change it. */
    aiConfig: () => request<AiConfigView>('/api/v1/ai/config'),
    /** Forwards one non-streaming chat completion through the host. */
    aiChat: (body: AiChatRequest) =>
      request<unknown>('/api/v1/ai/chat', { method: 'POST', body: JSON.stringify(body) }),
    /**
     * Forwards one streaming chat completion. The host answers
     * `text/event-stream` and this decodes each raw chunk as UTF-8 (a multi-byte
     * character may be split across chunks, hence `{ stream: true }`); it does
     * not parse SSE. `signal` aborts both the local read and the upstream call.
     */
    aiChatStream: async (
      body: AiChatRequest,
      onChunk: (text: string) => void,
      signal?: AbortSignal,
    ): Promise<void> => {
      const response = await doFetch(`${base}/api/v1/ai/chat`, {
        method: 'POST',
        headers: authHeaders(),
        body: JSON.stringify({ ...body, stream: true }),
        signal,
      })
      if (!response.ok) await raiseProblem(response)
      const stream = response.body
      if (stream === null) return
      const reader = stream.getReader()
      const decoder = new TextDecoder()
      for (;;) {
        const { done, value } = await reader.read()
        if (done) break
        if (value !== undefined) onChunk(decoder.decode(value, { stream: true }))
      }
      // Flush a trailing multi-byte sequence the final chunk may have split.
      const tail = decoder.decode()
      if (tail !== '') onChunk(tail)
    },
  }
}

export type HttpService = ReturnType<typeof createHttpService>
