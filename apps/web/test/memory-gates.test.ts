// Unit tests for the Memory view availability gates (T20, §6.1): a host shows
// the view only when it declares the `memory.sample` capability *and* serves
// the snapshot method; the terminate gate additionally needs the flag and both
// plan/execute methods.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { Capabilities } from '@space-lens/contract'
import { memoryAvailable, memoryTerminateAvailable } from '../src/lib/gates.ts'

function capabilities(memory?: Capabilities['memory']): Capabilities {
  return {
    apiMajor: 1,
    contractVersion: '1.0.0',
    scan: { start: true, cancel: true, discovery: true, maxConcurrent: 1 },
    cleanup: { plan: true, execute: true, mode: 'trash' },
    host: { folderPicker: false },
    icloud: 'unavailable',
    ...(memory === undefined ? {} : { memory }),
  }
}

function memoryCapability(overrides: Partial<NonNullable<Capabilities['memory']>> = {}) {
  return {
    sample: true,
    terminate: true,
    gracefulAppQuit: true,
    reopen: true,
    platform: 'macos' as const,
    ...overrides,
  }
}

const fullService = {
  memorySnapshot: () => Promise.reject(new Error('unused')),
  memoryTerminatePlan: () => Promise.reject(new Error('unused')),
  memoryTerminateExecute: () => Promise.reject(new Error('unused')),
}

test('memory view needs the sample capability and the snapshot method', () => {
  assert.equal(memoryAvailable({ capabilities: capabilities(memoryCapability()), service: fullService }), true)
  // capability absent → hidden (older hosts / machine mode)
  assert.equal(memoryAvailable({ capabilities: capabilities(), service: fullService }), false)
  // sample false → hidden
  assert.equal(
    memoryAvailable({ capabilities: capabilities(memoryCapability({ sample: false })), service: fullService }),
    false,
  )
  // method missing → hidden
  assert.equal(memoryAvailable({ capabilities: capabilities(memoryCapability()), service: {} }), false)
  assert.equal(memoryAvailable({ capabilities: capabilities(memoryCapability()), service: null }), false)
  assert.equal(memoryAvailable({ capabilities: null, service: fullService }), false)
})

test('terminate needs the flag plus both plan and execute methods', () => {
  assert.equal(
    memoryTerminateAvailable({ capabilities: capabilities(memoryCapability()), service: fullService }),
    true,
  )
  // terminate false → read-only
  assert.equal(
    memoryTerminateAvailable({
      capabilities: capabilities(memoryCapability({ terminate: false })),
      service: fullService,
    }),
    false,
  )
  // missing plan arm
  assert.equal(
    memoryTerminateAvailable({
      capabilities: capabilities(memoryCapability()),
      service: { memorySnapshot: fullService.memorySnapshot, memoryTerminateExecute: fullService.memoryTerminateExecute },
    }),
    false,
  )
  // missing execute arm
  assert.equal(
    memoryTerminateAvailable({
      capabilities: capabilities(memoryCapability()),
      service: { memorySnapshot: fullService.memorySnapshot, memoryTerminatePlan: fullService.memoryTerminatePlan },
    }),
    false,
  )
  // sample false hides terminate too
  assert.equal(
    memoryTerminateAvailable({
      capabilities: capabilities(memoryCapability({ sample: false })),
      service: fullService,
    }),
    false,
  )
})
