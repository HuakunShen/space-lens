import type { Problem, ProblemCode } from '@space-lens/contract'
import { problem } from '@space-lens/contract'

const STATUS_BY_CODE: Record<ProblemCode, number> = {
  Unauthenticated: 401,
  Forbidden: 403,
  NotFound: 404,
  InvalidRequest: 400,
  UnsupportedOperation: 400,
  LimitExceeded: 429,
  StalePlan: 409,
  StaleSnapshot: 409,
  Conflict: 409,
  Timeout: 504,
  Cancelled: 409,
  Unavailable: 503,
  InternalError: 500,
}

export class ProblemError extends Error {
  readonly problem: Problem
  readonly status: number

  constructor(problem: Problem, status?: number) {
    super(problem.message)
    this.problem = problem
    this.status = status ?? STATUS_BY_CODE[problem.code] ?? 500
  }
}

export const Problems = {
  unauthenticated: (message = 'Authentication required.') =>
    new ProblemError(problem('Unauthenticated', message, { retryable: false })),
  forbidden: (message = 'This session lacks the required scope.') => new ProblemError(problem('Forbidden', message)),
  notFound: (message = 'No such resource.') => new ProblemError(problem('NotFound', message)),
  invalid: (message: string, details?: Record<string, string>) =>
    new ProblemError(problem('InvalidRequest', message, { details })),
  unsupported: (message: string) => new ProblemError(problem('UnsupportedOperation', message)),
  limit: (message: string, retryable = true) => new ProblemError(problem('LimitExceeded', message, { retryable })),
  stalePlan: (message: string) => new ProblemError(problem('StalePlan', message, { retryable: false })),
  staleSnapshot: (message: string) => new ProblemError(problem('StaleSnapshot', message, { retryable: false })),
  conflict: (message: string) => new ProblemError(problem('Conflict', message)),
  timeout: (message: string, retryable = true) => new ProblemError(problem('Timeout', message, { retryable })),
  cancelled: (message = 'The operation was cancelled.') => new ProblemError(problem('Cancelled', message)),
  unavailable: (message: string, retryable = true) => new ProblemError(problem('Unavailable', message, { retryable })),
  internal: (message: string) => new ProblemError(problem('InternalError', message, { retryable: false })),
}
