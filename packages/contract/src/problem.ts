import { z } from 'zod'

/**
 * Closed problem-code enum. Hosts must never invent codes: an unknown failure
 * is `InternalError` carrying redacted facts, never a guessed classification.
 */
export const PROBLEM_CODES = [
  'Unauthenticated',
  'Forbidden',
  'NotFound',
  'InvalidRequest',
  'UnsupportedOperation',
  'LimitExceeded',
  'StalePlan',
  'StaleSnapshot',
  'Conflict',
  'Timeout',
  'Cancelled',
  'Unavailable',
  'InternalError',
] as const

export type ProblemCode = (typeof PROBLEM_CODES)[number]

export const ProblemSchema = z.strictObject({
  code: z.enum(PROBLEM_CODES),
  message: z.string().min(1).max(2000),
  /** Flat, redacted facts. Never file contents, never stack traces. */
  details: z.record(z.string(), z.string()).optional(),
  retryable: z.boolean(),
})

export type Problem = z.infer<typeof ProblemSchema>

export function problem(
  code: ProblemCode,
  message: string,
  options?: { retryable?: boolean; details?: Record<string, string> },
): Problem {
  return { code, message, retryable: options?.retryable ?? false, details: options?.details }
}
