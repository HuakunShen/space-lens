/**
 * Memory view and process-control contract tests (T1, §4.3/§4.4). A single
 * fixture file (`crates/memory/tests/fixtures/contract-sample.json`) holds one
 * valid instance per wire schema; this file checks it parses here, and the Rust
 * crate round-trips the same file in T2, so either side drifting fails a test.
 */
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

import {
  API_MAJOR,
  CONTRACT_SCHEMAS,
  CONTRACT_VERSION,
  CapabilitiesSchema,
  MemorySnapshotSchema,
  SessionSchema,
  TerminateExecuteRequestSchema,
  TerminatePlanRequestSchema,
} from '../src/index.ts'

const here = dirname(fileURLToPath(import.meta.url))

/** Every schema name this task adds, in the order registered in CONTRACT_SCHEMAS. */
const MEMORY_SCHEMA_NAMES = [
  'MemoryBreakdown',
  'MemoryPressure',
  'ProcessRow',
  'ProcessGroup',
  'MemorySnapshot',
  'TerminateTarget',
  'TerminatePlanRequest',
  'TerminatePlanEntry',
  'TerminateRefusal',
  'TerminatePlan',
  'TerminateExecuteRequest',
  'TerminateResult',
  'TerminateOutcome',
  'CandidateReason',
  'FreeUpCandidate',
  'FreeUpCandidates',
  'CandidatesRequest',
  'SystemPoint',
  'GroupSeries',
  'MemoryHistory',
  'HistoryRequest',
  'AuditRecord',
  'AuditLog',
] as const

const fixturePath = join(here, '..', '..', '..', 'crates', 'memory', 'tests', 'fixtures', 'contract-sample.json')
const fixture = JSON.parse(readFileSync(fixturePath, 'utf8')) as Record<string, unknown>

describe('memory contract fixture', () => {
  it('holds exactly one instance per memory schema, and each parses', () => {
    expect(Object.keys(fixture).sort()).toEqual([...MEMORY_SCHEMA_NAMES].sort())
    for (const name of MEMORY_SCHEMA_NAMES) {
      const schema = CONTRACT_SCHEMAS[name]
      expect(schema, `${name} is registered in CONTRACT_SCHEMAS`).toBeDefined()
      expect(schema.safeParse(fixture[name]).success, `${name} parses its fixture instance`).toBe(true)
    }
  })

  it('parses a full MemorySnapshot', () => {
    const snapshot = MemorySnapshotSchema.parse(fixture.MemorySnapshot)
    expect(snapshot.sequence).toBe(3)
    expect(snapshot.groups).toHaveLength(1)
    expect(snapshot.processes[0]?.name).toBe('Dia')
  })
})

describe('memory request guards', () => {
  const planId = 'tplan_0123456789ABCDEF'

  it('requires the literal confirm on execute', () => {
    expect(TerminateExecuteRequestSchema.safeParse({ planId, confirm: true }).success).toBe(true)
    expect(TerminateExecuteRequestSchema.safeParse({ planId, confirm: false }).success).toBe(false)
    expect(TerminateExecuteRequestSchema.safeParse({ planId }).success).toBe(false)
  })

  it('rejects an extra field on a plan request', () => {
    const request = {
      targets: [{ kind: 'group', groupId: 'grp_0123456789abcdef' }],
      mode: 'graceful',
      origin: 'manual',
    }
    expect(TerminatePlanRequestSchema.safeParse(request).success).toBe(true)
    expect(TerminatePlanRequestSchema.safeParse({ ...request, extra: true }).success).toBe(false)
  })

  it('bounds targets to 20', () => {
    const targets = Array.from({ length: 21 }, () => ({ kind: 'group', groupId: 'grp_0123456789abcdef' }))
    expect(TerminatePlanRequestSchema.safeParse({ targets, mode: 'force', origin: 'manual' }).success).toBe(false)
    expect(
      TerminatePlanRequestSchema.safeParse({ targets: targets.slice(0, 20), mode: 'force', origin: 'manual' }).success,
    ).toBe(true)
  })
})

describe('memory scopes and capabilities', () => {
  it('accepts the memory scopes on a session', () => {
    const session = {
      token: 'x'.repeat(32),
      sessionId: 'sess_1',
      expiresAt: '2026-10-18T00:00:00.000Z',
      scopes: ['ai:chat', 'memory:read'],
    }
    expect(SessionSchema.parse(session).scopes).toEqual(['ai:chat', 'memory:read'])
    expect(SessionSchema.safeParse({ ...session, scopes: ['memory:terminate'] }).success).toBe(true)
    expect(SessionSchema.safeParse({ ...session, scopes: ['memory:write'] }).success).toBe(false)
  })

  it('carries memory as an optional capability and keeps it closed', () => {
    const capabilities = {
      apiMajor: API_MAJOR,
      contractVersion: CONTRACT_VERSION,
      scan: { start: true, cancel: true, maxConcurrent: 1 },
      cleanup: { plan: false, execute: false, mode: 'none' as const },
      host: { folderPicker: false },
      icloud: 'unavailable' as const,
    }
    expect(CapabilitiesSchema.parse(capabilities).memory).toBeUndefined()
    const memory = {
      sample: true,
      terminate: true,
      gracefulAppQuit: true,
      reopen: true,
      platform: 'macos' as const,
    }
    expect(CapabilitiesSchema.parse({ ...capabilities, memory }).memory).toEqual(memory)
    expect(CapabilitiesSchema.safeParse({ ...capabilities, memory: { ...memory, platform: 'beos' } }).success).toBe(
      false,
    )
    expect(CapabilitiesSchema.safeParse({ ...capabilities, memory: { ...memory, extra: true } }).success).toBe(false)
  })
})
