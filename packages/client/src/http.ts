import type {
  Capabilities,
  ChildrenPage,
  ChildrenPageRequest,
  CleanupExecuteRequest,
  CleanupOutcome,
  CleanupPlan,
  CleanupPlanRequest,
  Health,
  Problem,
  RootsResponse,
  ScanListResponse,
  ScanSession,
  ScanStartRequest,
  ScanStatus,
  Session,
  TreeSlice,
  TreeSliceRequest,
} from '@space-lens/contract'

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

  const request = async <T>(path: string, init?: RequestInit): Promise<T> => {
    const token = options.getToken()
    const headers: Record<string, string> = {
      'content-type': 'application/json',
      ...(init?.headers as Record<string, string>),
    }
    if (token !== null) headers.authorization = `Bearer ${token}`
    const response = await doFetch(`${base}${path}`, { ...init, headers })
    if (!response.ok) await raiseProblem(response)
    if (response.status === 204) return undefined as T
    return (await response.json()) as T
  }

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
    startScan: (body: ScanStartRequest) =>
      request<ScanSession>('/api/v1/scans', { method: 'POST', body: JSON.stringify(body) }),
    scanList: () => request<ScanListResponse>('/api/v1/scans'),
    scanStatus: (scanId: string) => request<ScanStatus>(`/api/v1/scans/${scanId}`),
    cancelScan: (scanId: string) =>
      request<ScanStatus>(`/api/v1/scans/${scanId}/cancel`, { method: 'POST', body: '{}' }),
    treeSlice: (body: TreeSliceRequest) =>
      request<TreeSlice>('/api/v1/tree/slice', { method: 'POST', body: JSON.stringify(body) }),
    children: (body: ChildrenPageRequest) =>
      request<ChildrenPage>('/api/v1/tree/children', { method: 'POST', body: JSON.stringify(body) }),
    plan: (body: CleanupPlanRequest) =>
      request<CleanupPlan>('/api/v1/cleanup/plan', { method: 'POST', body: JSON.stringify(body) }),
    execute: (body: CleanupExecuteRequest) =>
      request<CleanupOutcome>('/api/v1/cleanup/execute', { method: 'POST', body: JSON.stringify(body) }),
  }
}

export type HttpService = ReturnType<typeof createHttpService>
