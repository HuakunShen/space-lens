import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

import {
  buildContractArtifacts,
  CleanupExecuteRequestSchema,
  CleanupPlanRequestSchema,
  DiscoveryRequestSchema,
  DiscoveryPageSchema,
  EventEnvelopeSchema,
  ProblemSchema,
  ScanStartRequestSchema,
  TreeSliceSchema,
  LocalScanReportSchema,
  LocalScanProgressSchema,
  LocalScanMessageSchema,
} from '../src/index.ts'

const here = dirname(fileURLToPath(import.meta.url))
const generatedPath = join(here, '..', 'generated', 'contract.schema.json')

describe('discovery contract', () => {
  it('defaults paging and rejects open or unbounded requests', () => {
    expect(DiscoveryRequestSchema.parse({ scanId: 'scan_abcdefgh', kind: 'caches' })).toEqual({
      scanId: 'scan_abcdefgh',
      kind: 'caches',
      minSize: 0,
      offset: 0,
      limit: 200,
    })
    for (const invalid of [
      { kind: 'other' },
      { minSize: -1 },
      { offset: 0.5 },
      { limit: 0 },
      { limit: 1001 },
      { extra: true },
    ]) {
      expect(DiscoveryRequestSchema.safeParse({ scanId: 'scan_abcdefgh', kind: 'caches', ...invalid }).success).toBe(
        false,
      )
    }
    expect(
      DiscoveryPageSchema.safeParse({
        scanId: 'scan_abcdefgh',
        kind: 'caches',
        items: [],
        total: 0,
        totalSize: 0,
        offset: 0,
        limit: 200,
      }).success,
    ).toBe(true)
  })
})

describe('scan contract', () => {
  it('validates preorder node frames without permitting nested payload trees', () => {
    const node = {
      name: 'local',
      path: '/tmp/local',
      size: 4096,
      children: [],
      depth: 0,
      ignored: false,
      collapsed: false,
      logicalSize: 1,
      isDirectory: true,
      scanState: 'complete',
      skipReason: null,
    }
    expect(LocalScanMessageSchema.safeParse({ type: 'node', node }).success).toBe(true)
    expect(LocalScanMessageSchema.safeParse({ type: 'node', node: { ...node, children: [node] } }).success).toBe(false)
  })
  it('accepts protected requests and validates coverage instead of accepting empty success', () => {
    expect(ScanStartRequestSchema.parse({ paths: ['/tmp/local'], localOnly: true }).localOnly).toBe(true)
    expect(
      LocalScanProgressSchema.safeParse({
        currentPath: '/tmp/local',
        bytesScanned: 12,
        entriesScanned: 2,
        files: 1,
        directories: 1,
        skippedCount: 0,
        deniedCount: 0,
        elapsedMs: 10,
      }).success,
    ).toBe(true)
    expect(LocalScanReportSchema.safeParse({ nodes: [], coverage: { mode: 'local-only' }, volumes: [] }).success).toBe(
      false,
    )
  })
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
