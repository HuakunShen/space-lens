import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

import {
  buildContractArtifacts,
  CleanupExecuteRequestSchema,
  CleanupPlanRequestSchema,
  EventEnvelopeSchema,
  ProblemSchema,
  ScanStartRequestSchema,
  TreeSliceSchema,
} from '../src/index.ts'

const here = dirname(fileURLToPath(import.meta.url))
const generatedPath = join(here, '..', 'generated', 'contract.schema.json')

describe('scan contract', () => {
  it('fills defaults and accepts a minimal scan start', () => {
    const parsed = ScanStartRequestSchema.parse({ paths: ['/tmp/demo'] })
    expect(parsed).toEqual({
      paths: ['/tmp/demo'],
      ignoreHidden: false,
      respectGitignore: true,
      ignoredMode: 'summarize',
    })
  })

  it('rejects unknown fields and bad payloads', () => {
    expect(ScanStartRequestSchema.safeParse({ paths: ['/tmp'], extra: 1 }).success).toBe(false)
    expect(ScanStartRequestSchema.safeParse({ paths: [] }).success).toBe(false)
    expect(ScanStartRequestSchema.safeParse({ paths: ['/tmp'], ignoredMode: 'everything' }).success).toBe(false)
  })
})

describe('cleanup contract', () => {
  it('requires the literal confirm', () => {
    expect(CleanupExecuteRequestSchema.safeParse({ planId: 'plan_abcdefgh', confirm: true }).success).toBe(true)
    expect(CleanupExecuteRequestSchema.safeParse({ planId: 'plan_abcdefgh', confirm: false }).success).toBe(false)
    expect(CleanupExecuteRequestSchema.safeParse({ planId: 'plan_abcdefgh' }).success).toBe(false)
  })

  it('bounds plan requests', () => {
    expect(CleanupPlanRequestSchema.safeParse({ scanId: 'scan_abcdefgh', nodeIds: [] }).success).toBe(false)
  })
})

describe('tree slice contract', () => {
  it('parses a recursive slice', () => {
    const slice = {
      scanId: 'scan_abcdefgh',
      focusNode: {
        id: 'abc',
        name: 'root',
        path: '/tmp/root',
        size: 10,
        depth: 0,
        ignored: false,
        collapsed: false,
        hasChildren: true,
        childCount: 1,
      },
      ancestors: [],
      tree: {
        id: 'abc',
        name: 'root',
        path: '/tmp/root',
        size: 10,
        depth: 0,
        ignored: false,
        collapsed: false,
        hasChildren: true,
        childCount: 1,
        children: [
          {
            id: 'def',
            name: 'child',
            path: '/tmp/root/child',
            size: 4,
            depth: 1,
            ignored: false,
            collapsed: false,
            hasChildren: false,
            childCount: 0,
            children: [],
            omittedBytes: 0,
            omittedCount: 0,
          },
        ],
        omittedBytes: 6,
        omittedCount: 1,
      },
      totalSize: 10,
      truncated: false,
      omittedBytes: 6,
      omittedCount: 1,
      generatedAt: '2026-01-01T00:00:00.000Z',
    }
    expect(TreeSliceSchema.parse(slice)).toEqual(slice)
  })
})

describe('events contract', () => {
  it('discriminates payload kinds', () => {
    const envelope = {
      sequence: 3,
      emittedAt: '2026-01-01T00:00:00.000Z',
      payload: {
        kind: 'scan.completed',
        status: {
          scanId: 'scan_abcdefgh',
          state: 'ready',
          message: '',
          progress: null,
          currentPath: null,
          bytesScanned: 0,
          entriesScanned: 0,
          rootIds: [],
          label: null,
          updatedAt: '2026-01-01T00:00:00.000Z',
        },
      },
    }
    const parsed = EventEnvelopeSchema.parse(envelope)
    expect(parsed.payload.kind).toBe('scan.completed')
    expect(EventEnvelopeSchema.safeParse({ ...envelope, payload: { kind: 'nope' } }).success).toBe(false)
  })
})

describe('problem contract', () => {
  it('keeps the envelope closed', () => {
    expect(ProblemSchema.safeParse({ code: 'NotFound', message: 'missing', retryable: false }).success).toBe(true)
    expect(ProblemSchema.safeParse({ code: 'Whatever', message: 'x', retryable: false }).success).toBe(false)
    expect(ProblemSchema.safeParse({ code: 'NotFound', message: '', retryable: false }).success).toBe(false)
  })
})

describe('generated artifacts', () => {
  it('are deterministic', () => {
    const first = buildContractArtifacts()
    const second = buildContractArtifacts()
    expect(first).toEqual(second)
  })

  it('match the committed file (run `yarn workspace @space-lens/contract generate` after schema changes)', () => {
    const built = buildContractArtifacts()['contract.schema.json']
    if (process.env.UPDATE_CONTRACT_ARTIFACTS === '1' || !existsSync(generatedPath)) {
      writeFileSync(generatedPath, built)
    }
    expect(readFileSync(generatedPath, 'utf8')).toBe(built)
  })
})
