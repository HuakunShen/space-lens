/**
 * Tests for the host's `spacelens memory stdio` bridge (T13 of
 * docs/memory-cleaner-plan.md). The child is the fake script under
 * `test/fixtures/fake-memory-child.mjs`, so no real process scan, AppKit call or
 * signal ever runs here: every case exercises the bridge's own framing —
 * lazy spawn, id correlation, the 4 MiB line cap, problem mapping, timeout,
 * child exit and respawn.
 */

import { spawn, type ChildProcess, type SpawnOptions } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, describe, expect, it } from 'vitest'

import { MEMORY_PLATFORMS } from '@space-lens/contract'
import { MAX_LINE_BYTES, createMemoryBridge, type MemoryBridge, type MemoryCall } from '../src/memory.ts'
import { ProblemError } from '../src/problems.ts'

const FIXTURE = join(dirname(fileURLToPath(import.meta.url)), 'fixtures', 'fake-memory-child.mjs')
const GROUP = 'grp_0123456789abcdef'
const PLAN = 'tplan_0123456789abcdef'

type BridgeOptions = Parameters<typeof createMemoryBridge>[0]

interface SpawnRecord {
  command: string
  args: readonly string[]
  options: SpawnOptions
  child: ChildProcess
}

/** The real spawn, recording every call so tests can assert laziness and respawn. */
function recordingSpawn(records: SpawnRecord[]): BridgeOptions['spawnImpl'] {
  return ((command: string, args: readonly string[], options: SpawnOptions) => {
    const child = spawn(command, args as string[], options)
    records.push({ command, args, options, child })
    return child
  }) as BridgeOptions['spawnImpl']
}

const bridges: MemoryBridge[] = []

/** Create a bridge whose fake child runs `mode`, scheduled for cleanup. */
function bridgeFor(mode: string, options: Partial<BridgeOptions> = {}): MemoryBridge {
  process.env.FAKE_MEMORY_MODE = mode
  const bridge = createMemoryBridge({ binary: process.execPath, args: [FIXTURE], ...options })
  bridges.push(bridge)
  return bridge
}

/** Await a promise that must reject with a `ProblemError`, and return it. */
async function rejectionOf(promise: Promise<unknown>): Promise<ProblemError> {
  let caught: unknown
  try {
    await promise
  } catch (error) {
    caught = error
  }
  if (!(caught instanceof ProblemError)) throw new Error(`expected a ProblemError rejection, got ${String(caught)}`)
  return caught
}

afterEach(async () => {
  delete process.env.FAKE_MEMORY_MODE
  await Promise.all(bridges.splice(0).map((bridge) => bridge.stop().catch(() => {})))
})

describe('createMemoryBridge', () => {
  it('spawns lazily, reads hello first, and correlates replies by id', async () => {
    const records: SpawnRecord[] = []
    const bridge = bridgeFor('echo', { spawnImpl: recordingSpawn(records) })

    expect(records).toHaveLength(0)
    await bridge.hello()
    await bridge.hello()
    expect(records).toHaveLength(1)

    const hello = await bridge.hello()
    expect(MEMORY_PLATFORMS).toContain(hello.platform)
    expect(hello).toEqual({ platform: 'macos', terminate: true, gracefulAppQuit: true })

    expect(records[0].command).toBe(process.execPath)
    expect(records[0].args).toEqual([FIXTURE, 'memory', 'stdio'])
    expect(records[0].options).toMatchObject({ shell: false, windowsHide: true })

    const result = await bridge.call('sess_one', { method: 'snapshot' })
    expect(result).toEqual({ method: 'snapshot', sessionId: 'sess_one', request: null })
  })

  it('forwards every method with its session id and request payload', async () => {
    const bridge = bridgeFor('echo')
    await bridge.hello()

    const calls: MemoryCall[] = [
      { method: 'snapshot' },
      { method: 'history', request: { groupIds: [GROUP] } },
      { method: 'candidates', request: { exclude: [GROUP] } },
      { method: 'audit' },
      {
        method: 'plan',
        request: { targets: [{ kind: 'group', groupId: GROUP }], mode: 'graceful', origin: 'manual' },
      },
      { method: 'execute', request: { planId: PLAN, confirm: true } },
    ]

    for (const call of calls) {
      const result = await bridge.call('sess_two', call)
      expect(result).toMatchObject({ method: call.method, sessionId: 'sess_two' })
      if ('request' in call) expect(result).toMatchObject({ request: call.request })
      else expect(result).toMatchObject({ request: null })
    }
  })

  it('maps a child problem reply onto a ProblemError with the same code and status', async () => {
    const bridge = bridgeFor('problem')
    // The problem arrives before hello is even awaited: the bridge must still
    // resolve hello from the first line and reject the call.
    const error = await rejectionOf(bridge.call('sess', { method: 'snapshot' }))
    expect(error).toBeInstanceOf(ProblemError)
    expect(error.problem.code).toBe('LimitExceeded')
    expect(error.problem.retryable).toBe(true)
    expect(error.status).toBe(429)
  })

  it('fails a call as Timeout when the child never replies', async () => {
    const bridge = bridgeFor('timeout', { callTimeoutMs: 60 })
    await bridge.hello()
    const error = await rejectionOf(bridge.call('sess', { method: 'snapshot' }))
    expect(error.problem.code).toBe('Timeout')
    expect(error.problem.retryable).toBe(true)
    expect(error.status).toBe(504)
  })

  it('rejects a pending call with Unavailable and respawns after the child exits', async () => {
    const records: SpawnRecord[] = []
    const bridge = bridgeFor('exit', { spawnImpl: recordingSpawn(records) })
    await bridge.hello()

    expect((await rejectionOf(bridge.call('sess', { method: 'snapshot' }))).problem.code).toBe('Unavailable')

    await bridge.hello()
    expect(records).toHaveLength(2)
    expect((await rejectionOf(bridge.call('sess', { method: 'snapshot' }))).problem.code).toBe('Unavailable')
  })

  it('kills the child and rejects every pending call when a line exceeds 4 MiB', async () => {
    expect(MAX_LINE_BYTES).toBe(4 * 1024 * 1024)
    const bridge = bridgeFor('overflow', { callTimeoutMs: 5000 })
    await bridge.hello()

    const results = await Promise.allSettled([
      bridge.call('sess', { method: 'snapshot' }),
      bridge.call('sess', { method: 'audit' }),
    ])
    for (const result of results) {
      expect(result.status).toBe('rejected')
      if (result.status !== 'rejected') continue
      expect(result.reason).toBeInstanceOf(ProblemError)
      if (result.reason instanceof ProblemError) expect(result.reason.problem.code).toBe('InternalError')
    }
  })

  it('rejects an in-flight call and exits the child on stop', async () => {
    const records: SpawnRecord[] = []
    const bridge = bridgeFor('timeout', { spawnImpl: recordingSpawn(records), callTimeoutMs: 30_000 })
    await bridge.hello()

    const inFlight = rejectionOf(bridge.call('sess', { method: 'snapshot' }))
    await bridge.stop()
    expect((await inFlight).problem.code).toBe('Unavailable')
    expect(records[0].child.exitCode).not.toBeNull()
  })

  it('resolves stop immediately when no child was ever spawned', async () => {
    const bridge = createMemoryBridge({ binary: process.execPath, args: [FIXTURE] })
    await expect(bridge.stop()).resolves.toBeUndefined()
  })

  it('fails hello when the child exits before writing it', async () => {
    const bridge = bridgeFor('exit-before-hello')
    expect((await rejectionOf(bridge.hello())).problem.code).toBe('Unavailable')
  })

  it('fails hello when the child writes a malformed hello', async () => {
    const bridge = bridgeFor('hello-invalid')
    expect((await rejectionOf(bridge.hello())).problem.code).toBe('InternalError')
  })
})
