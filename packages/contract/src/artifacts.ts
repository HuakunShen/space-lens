import { z } from 'zod'
import { CapabilitiesSchema, HealthSchema } from './capabilities.ts'
import {
  CleanupExecuteRequestSchema,
  CleanupOutcomeSchema,
  CleanupPlanRequestSchema,
  CleanupPlanSchema,
} from './cleanup.ts'
import { EventEnvelopeSchema } from './events.ts'
import { ProblemSchema } from './problem.ts'
import { ScanStartRequestSchema, ScanStatusSchema, TreeSliceSchema, TreeSliceRequestSchema } from './scan.ts'
import { SessionExchangeRequestSchema, SessionSchema } from './session.ts'
import { API_MAJOR, CONTRACT_VERSION } from './version.ts'

/**
 * The named public surface of the contract. Every schema listed here is
 * emitted into generated/contract.schema.json; adding a schema without
 * regenerating fails `check:contract`, which keeps the committed artifact the
 * single rendered source of truth.
 */
export const CONTRACT_SCHEMAS: Record<string, z.ZodType> = {
  Health: HealthSchema,
  Capabilities: CapabilitiesSchema,
  ScanStartRequest: ScanStartRequestSchema,
  ScanStatus: ScanStatusSchema,
  TreeSliceRequest: TreeSliceRequestSchema,
  TreeSlice: TreeSliceSchema,
  CleanupPlanRequest: CleanupPlanRequestSchema,
  CleanupPlan: CleanupPlanSchema,
  CleanupExecuteRequest: CleanupExecuteRequestSchema,
  CleanupOutcome: CleanupOutcomeSchema,
  SessionExchangeRequest: SessionExchangeRequestSchema,
  Session: SessionSchema,
  EventEnvelope: EventEnvelopeSchema,
  Problem: ProblemSchema,
}

export function buildContractArtifacts(): Record<string, string> {
  const defs: Record<string, unknown> = {}
  for (const [name, schema] of Object.entries(CONTRACT_SCHEMAS)) {
    defs[name] = z.toJSONSchema(schema, { io: 'output', unrepresentable: 'any', cycles: 'ref' })
  }
  const document = {
    $schema: 'http://json-schema.org/draft-07/schema#',
    title: 'space-lens contract',
    apiMajor: API_MAJOR,
    contractVersion: CONTRACT_VERSION,
    $defs: defs,
  }
  return { 'contract.schema.json': `${JSON.stringify(document, null, 2)}\n` }
}
