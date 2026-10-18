/**
 * The host's bridge to `spacelens memory stdio` (T13 of
 * docs/memory-cleaner-plan.md, §4.6). The HTTP host never re-implements a
 * memory rule: it spawns the release CLI as one long-lived child and forwards
 * each call over the line protocol, so plans, history and rate limits all live
 * in the Rust process.
 *
 * The bridge owns exactly one child, spawned lazily on the first call. Requests
 * are correlated by a monotonic id; replies either resolve their call or reject
 * it with the child's problem (the same closed code, status from the host's
 * `Problems` helpers). A reply line is capped at 4 MiB: a longer one kills the
 * child and rejects everything pending. A call that gets no reply within
 * `callTimeoutMs` becomes a `Timeout`; a child that exits rejects the pending
 * calls with `Unavailable` and is respawned on the next call.
 */

import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process'
import {
  MEMORY_PLATFORMS,
  PROBLEM_CODES,
  type CandidatesRequest,
  type HistoryRequest,
  type ProblemCode,
  type TerminateExecuteRequest,
  type TerminatePlanRequest,
} from '@space-lens/contract'
import { ProblemError, Problems } from './problems.ts'

/** The platform literal the child reports in `hello`. */
export type MemoryPlatform = (typeof MEMORY_PLATFORMS)[number]

/** One forwarded call, tagged on `method` exactly as the child's `Call` enum. */
export type MemoryCall =
  | { method: 'snapshot' }
  | { method: 'history'; request: HistoryRequest }
  | { method: 'candidates'; request: CandidatesRequest }
  | { method: 'audit' }
  | { method: 'plan'; request: TerminatePlanRequest }
  | { method: 'execute'; request: TerminateExecuteRequest }

/** The capability line the child writes once, before any reply. */
export interface MemoryHello {
  platform: MemoryPlatform
  terminate: boolean
  gracefulAppQuit: boolean
}

/** The one-child, id-correlated protocol client. */
export interface MemoryBridge {
  hello(): Promise<MemoryHello>
  call(sessionId: string, call: MemoryCall): Promise<unknown>
  stop(): Promise<void>
}

/** The most bytes one reply line may take (§4.6). */
export const MAX_LINE_BYTES = 4 * 1024 * 1024
/** How long a call waits for its reply before failing as `Timeout`. */
export const DEFAULT_CALL_TIMEOUT_MS = 15_000
/** How long `stop` waits for the child to honour `shutdown` before it is killed. */
const SHUTDOWN_GRACE_MS = 2_000

/** The status each problem code carries, sourced from the existing host helpers. */
type ProblemFactory = (message: string, retryable: boolean) => ProblemError
const PROBLEM_BY_CODE: Record<ProblemCode, ProblemFactory> = {
  Unauthenticated: (message) => Problems.unauthenticated(message),
  Forbidden: (message) => Problems.forbidden(message),
  NotFound: (message) => Problems.notFound(message),
  InvalidRequest: (message) => Problems.invalid(message),
  UnsupportedOperation: (message) => Problems.unsupported(message),
  LimitExceeded: (message, retryable) => Problems.limit(message, retryable),
  StalePlan: (message) => Problems.stalePlan(message),
  StaleSnapshot: (message) => Problems.staleSnapshot(message),
  Conflict: (message) => Problems.conflict(message),
  Timeout: (message, retryable) => Problems.timeout(message, retryable),
  Cancelled: (message) => Problems.cancelled(message),
  Unavailable: (message, retryable) => Problems.unavailable(message, retryable),
  InternalError: (message) => Problems.internal(message),
}

interface PendingCall {
  resolve: (value: unknown) => void
  reject: (error: unknown) => void
  timer: NodeJS.Timeout
}

interface ChildState {
  proc: ChildProcessWithoutNullStreams
  hello: Promise<MemoryHello>
  resolveHello: (hello: MemoryHello) => void
  rejectHello: (error: unknown) => void
  exited: boolean
  buffer: Buffer
}

/** True for a plain JSON object, never an array or null. */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/** True when `value` is one of the wire platform literals. */
function isPlatform(value: unknown): value is MemoryPlatform {
  return typeof value === 'string' && MEMORY_PLATFORMS.some((platform) => platform === value)
}

/** True when `value` is one of the closed problem codes. */
function isProblemCode(value: string): value is ProblemCode {
  return PROBLEM_CODES.some((code) => code === value)
}

/** Validate the `hello` payload; a malformed one is an `InternalError`. */
function parseHello(raw: unknown): MemoryHello | null {
  if (!isRecord(raw)) return null
  const { platform, terminate, gracefulAppQuit } = raw
  if (!isPlatform(platform) || typeof terminate !== 'boolean' || typeof gracefulAppQuit !== 'boolean') return null
  return { platform, terminate, gracefulAppQuit }
}

/** Rebuild the child's problem through the host helpers so the status matches its code. */
function decodeProblem(raw: unknown): ProblemError {
  if (isRecord(raw)) {
    const { code, message, retryable } = raw
    if (typeof code === 'string' && isProblemCode(code) && typeof message === 'string' && message !== '') {
      const base = PROBLEM_BY_CODE[code](message, typeof retryable === 'boolean' ? retryable : false)
      return new ProblemError(base.problem, base.status)
    }
  }
  return Problems.internal('The memory process sent a malformed problem reply.')
}

/**
 * Create the bridge. Nothing is spawned until the first `hello()` or `call()`,
 * so a host with the memory scopes disabled costs nothing.
 */
export function createMemoryBridge(options: {
  binary: string
  args?: string[]
  callTimeoutMs?: number
  spawnImpl?: typeof spawn
}): MemoryBridge {
  const args = options.args ?? []
  const callTimeoutMs = options.callTimeoutMs ?? DEFAULT_CALL_TIMEOUT_MS
  const spawnImpl = options.spawnImpl ?? spawn

  let state: ChildState | null = null
  let nextId = 1
  const pending = new Map<number, PendingCall>()

  /** Reject every pending call with `error` and stop their timers. */
  const failPending = (error: ProblemError): void => {
    for (const call of pending.values()) {
      clearTimeout(call.timer)
      call.reject(error)
    }
    pending.clear()
  }

  /** The child closed or failed to start; fail everything still waiting on it. */
  const handleExit = (child: ChildState): void => {
    if (child.exited) return
    child.exited = true
    if (state === child) state = null
    child.rejectHello(Problems.unavailable('The memory process exited before its hello reply.'))
    failPending(Problems.unavailable('The memory process exited before replying.'))
  }

  /** A reply line broke the 4 MiB cap; kill the child and fail everything. */
  const handleOverflow = (child: ChildState): void => {
    if (child.exited) return
    child.exited = true
    if (state === child) state = null
    child.proc.kill('SIGKILL')
    const detail = `The memory process sent a reply line over the ${MAX_LINE_BYTES} byte limit.`
    child.rejectHello(Problems.internal(detail))
    failPending(Problems.internal(detail))
  }

  /** Dispatch one complete reply line. */
  const handleLine = (child: ChildState, line: string): void => {
    const text = line.trim()
    if (text === '') return
    let decoded: unknown
    try {
      decoded = JSON.parse(text)
    } catch {
      return
    }
    if (!isRecord(decoded)) return

    if ('hello' in decoded) {
      const hello = parseHello(decoded.hello)
      if (hello === null) child.rejectHello(Problems.internal('The memory process sent a malformed hello reply.'))
      else child.resolveHello(hello)
      return
    }

    const { id } = decoded
    if (typeof id !== 'number') return
    const call = pending.get(id)
    if (call === undefined) return
    pending.delete(id)
    clearTimeout(call.timer)
    if (decoded.ok === true) call.resolve(decoded.result)
    else call.reject(decodeProblem(decoded.problem))
  }

  /** Split stdout into lines, enforcing the cap before anything is buffered past it. */
  const onData = (child: ChildState, chunk: Buffer): void => {
    if (child.exited) return
    child.buffer = Buffer.concat([child.buffer, chunk])
    for (;;) {
      const newline = child.buffer.indexOf(0x0a)
      if (newline === -1) {
        if (child.buffer.length > MAX_LINE_BYTES) handleOverflow(child)
        return
      }
      if (newline > MAX_LINE_BYTES) {
        handleOverflow(child)
        return
      }
      const line = child.buffer.subarray(0, newline).toString('utf8')
      child.buffer = child.buffer.subarray(newline + 1)
      handleLine(child, line)
      if (child.exited) return
    }
  }

  /** Spawn the child if needed and resolve with its `hello`. */
  const start = (): Promise<MemoryHello> => {
    if (state !== null) return state.hello
    const proc = spawnImpl(options.binary, [...args, 'memory', 'stdio'], { shell: false, windowsHide: true })
    let resolveHello: (hello: MemoryHello) => void = () => {}
    let rejectHello: (error: unknown) => void = () => {}
    const hello = new Promise<MemoryHello>((resolve, reject) => {
      resolveHello = resolve
      rejectHello = reject
    })
    const child: ChildState = { proc, hello, resolveHello, rejectHello, exited: false, buffer: Buffer.alloc(0) }
    state = child
    proc.stdout.on('data', (chunk: Buffer) => onData(child, chunk))
    proc.stderr.on('data', () => {})
    proc.stdin.on('error', () => {})
    proc.on('error', () => handleExit(child))
    proc.on('close', () => handleExit(child))
    return hello
  }

  const call = async (sessionId: string, request: MemoryCall): Promise<unknown> => {
    await start()
    if (state === null) throw Problems.unavailable('The memory process is not running.')
    const child = state
    const id = nextId
    nextId += 1
    return await new Promise<unknown>((resolve, reject) => {
      const timer = setTimeout(() => {
        pending.delete(id)
        reject(Problems.timeout(`The memory process did not reply within ${callTimeoutMs} ms.`))
      }, callTimeoutMs)
      timer.unref()
      pending.set(id, { resolve, reject, timer })
      child.proc.stdin.write(`${JSON.stringify({ id, sessionId, call: request })}\n`)
    })
  }

  const stop = async (): Promise<void> => {
    const child = state
    if (child === null) return
    state = null
    child.exited = true
    const stopped = Problems.unavailable('The memory bridge stopped.')
    child.rejectHello(stopped)
    failPending(stopped)
    try {
      child.proc.stdin.write(`${JSON.stringify({ id: nextId, sessionId: '', call: { method: 'shutdown' } })}\n`)
      child.proc.stdin.end()
    } catch {
      // The child already closed its input; the close handler below still runs.
    }
    await new Promise<void>((resolve) => {
      if (child.proc.exitCode !== null || child.proc.signalCode !== null) {
        resolve()
        return
      }
      const timer = setTimeout(() => {
        child.proc.kill('SIGKILL')
        resolve()
      }, SHUTDOWN_GRACE_MS)
      timer.unref()
      child.proc.once('close', () => {
        clearTimeout(timer)
        resolve()
      })
    })
  }

  return { hello: start, call, stop }
}
