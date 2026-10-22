import { accessSync, constants } from 'node:fs'
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process'
import { dirname, isAbsolute, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  LocalScanMessageSchema,
  type LocalScanProgress,
  type LocalScanReport,
  type LocalScanStreamNode,
  type ScanStartRequest,
} from '@space-lens/contract'
import { Problems } from './problems.ts'
import { ScanIndex } from './scan-index.ts'

export const LOCAL_SCAN_BIN_ENV = 'SPACLENS_LOCAL_SCAN_BIN'
const MAX_STDERR_BYTES = 8192
const MAX_LINE_BYTES = 512 * 1024 * 1024
const MAX_NODE_LINE_BYTES = 1024 * 1024

/** Only executable artifacts are probed here, never scan targets. */
export function resolveLocalScanBin(explicit?: string): string {
  const configured = explicit ?? process.env[LOCAL_SCAN_BIN_ENV]
  const valid = (candidate: string) => {
    try {
      accessSync(candidate, constants.X_OK)
      return true
    } catch {
      return false
    }
  }
  if (configured !== undefined) {
    if (!isAbsolute(configured) || !valid(configured)) {
      throw Problems.unavailable(
        `${LOCAL_SCAN_BIN_ENV} must name an existing absolute executable; protected scans never fall back`,
      )
    }
    return configured
  }
  const candidates = new Set<string>()
  for (const origin of [process.cwd(), dirname(fileURLToPath(import.meta.url)), dirname(process.execPath)]) {
    let cursor = resolve(origin)
    for (;;) {
      candidates.add(join(cursor, 'target', 'release', process.platform === 'win32' ? 'spacelens.exe' : 'spacelens'))
      const parent = dirname(cursor)
      if (parent === cursor) break
      cursor = parent
    }
  }
  for (const candidate of candidates) if (valid(candidate)) return candidate
  throw Problems.unavailable(
    `protected release scanner not found; build target/release/spacelens or set ${LOCAL_SCAN_BIN_ENV}; no legacy fallback`,
  )
}

/**
 * A streamed scan arrives already indexed and its report carries no nodes;
 * a non-streamed report carries the whole tree and `index` is null.
 */
export interface LocalScanResult {
  report: LocalScanReport
  index: ScanIndex | null
}

export interface LocalScanProcess {
  child: ChildProcessWithoutNullStreams
  done: Promise<LocalScanResult>
  terminate(): void
}

export function spawnLocalScan(
  binary: string,
  paths: string[],
  request: Pick<ScanStartRequest, 'ignoreHidden' | 'respectGitignore' | 'ignoredMode'>,
  onProgress: (progress: LocalScanProgress) => void,
): LocalScanProcess {
  const args = [
    'scan',
    ...paths,
    '--local-only',
    '--json',
    '--progress-json',
    '--stream-nodes',
    '--respect-gitignore',
    String(request.respectGitignore),
    '--ignored-mode',
    request.ignoredMode,
    ...(request.ignoreHidden ? ['--ignore-hidden'] : []),
  ]
  const child = spawn(binary, args, { shell: false, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true })
  child.stdin.end()
  let killTimer: ReturnType<typeof setTimeout> | undefined
  let ignoreOutput = false
  let discardPending = () => {}
  let discardTree = () => {}
  const terminate = () => {
    ignoreOutput = true
    discardPending()
    discardTree()
    if (child.exitCode !== null || child.signalCode !== null) return
    child.kill('SIGTERM')
    killTimer ??= setTimeout(() => child.kill('SIGKILL'), 2000)
    killTimer.unref()
  }
  const done = new Promise<LocalScanResult>((resolveDone, rejectDone) => {
    let parts: string[] = []
    let pendingBytes = 0
    let header = ''
    discardPending = () => {
      parts = []
      pendingBytes = 0
      header = ''
    }
    let stderr = Buffer.alloc(0)
    let report: LocalScanReport | null = null
    // Streamed nodes go straight into the compact index; no JS tree or set
    // of every path is kept. Duplicate paths are caught by the id table.
    let index = new ScanIndex(1 << 16)
    // Only the active preorder ancestors stay pinned, with their index rows.
    const ancestors: { node: LocalScanStreamNode; row: number }[] = []
    let streamed = false
    discardTree = () => {
      index = new ScanIndex()
      ancestors.length = 0
      report = null
    }
    let failure: Error | null = null
    const fail = (message: string) => {
      failure ??= new Error(message)
      terminate()
    }
    const attach = (node: LocalScanStreamNode) => {
      if (!isAbsolute(node.path) || resolve(node.path) !== node.path) {
        throw new Error('stream node path is non-absolute, unnormalized, or duplicated')
      }
      const cloud = /\/Library\/(?:CloudStorage|Mobile Documents)(?=\/|$)|\/Volumes\/GoogleDrive(?=\/|$)/.exec(
        node.path,
      )
      if (cloud !== null) {
        const cloudRoot = node.path.slice(0, cloud.index + cloud[0].length)
        if (node.path !== cloudRoot || node.scanState !== 'skipped') {
          throw new Error('stream cloud roots must be skipped and must not contain measured descendants')
        }
      }
      let parent: { node: LocalScanStreamNode; row: number } | null = null
      if (node.depth === 0) {
        if (!paths.includes(node.path)) throw new Error('stream root was not explicitly requested')
      } else {
        parent = node.depth <= ancestors.length ? ancestors[node.depth - 1] : null
        if (
          parent === null ||
          parent.node.path !== dirname(node.path) ||
          parent.node.depth + 1 !== node.depth ||
          !parent.node.isDirectory ||
          parent.node.scanState === 'skipped'
        ) {
          throw new Error('stream node has an absent, skipped, non-directory, or invalid preorder parent')
        }
      }
      const row = index.append(node, parent?.row ?? -1, parent?.node.path ?? null)
      if (row === -1) throw new Error('stream node path is non-absolute, unnormalized, or duplicated')
      ancestors.length = node.depth + 1
      ancestors[node.depth] = { node, row }
      streamed = true
    }
    const line = (input: string) => {
      if (failure !== null || input.trim() === '') return
      try {
        const decoded = JSON.parse(input)
        if (
          streamed &&
          decoded?.type === 'done' &&
          Array.isArray(decoded.report?.nodes) &&
          decoded.report.nodes.length !== 0
        ) {
          throw new Error('stream completion must not repeat the full tree')
        }
        if (decoded?.type === 'node' && Buffer.byteLength(input) > MAX_NODE_LINE_BYTES) {
          throw new Error('stream node line exceeds 1 MiB protocol limit')
        }
        const message = LocalScanMessageSchema.parse(decoded)
        if (report !== null) {
          fail('protected scanner emitted data after its final report')
          return
        }
        if (message.type === 'progress') onProgress(message.progress)
        else if (message.type === 'node') attach(message.node)
        else {
          if (streamed && message.report.nodes.length !== 0)
            throw new Error('stream completion must not repeat the full tree')
          // A streaming completion has no tree; its nodes are already indexed.
          report = message.report
        }
      } catch (error) {
        fail(
          `invalid protected scanner protocol: ${error instanceof Error ? error.message.slice(0, 1000) : String(error)}`,
        )
      }
    }
    child.stdout.setEncoding('utf8')
    child.stdout.on('data', (chunk: string) => {
      if (failure !== null || ignoreOutput) return
      let cursor = 0
      for (;;) {
        const newline = chunk.indexOf('\n', cursor)
        const piece = chunk.slice(cursor, newline === -1 ? undefined : newline)
        pendingBytes += Buffer.byteLength(piece)
        if (piece !== '') parts.push(piece)
        if (header.length < 256) header += piece.slice(0, 256 - header.length)
        const nodeLine = /^\s*\{\s*"node"\s*:/.test(header) || /^\s*\{\s*"type"\s*:\s*"node"/.test(header)
        if (pendingBytes > (nodeLine ? MAX_NODE_LINE_BYTES : MAX_LINE_BYTES)) {
          fail(
            nodeLine
              ? 'stream node line exceeds 1 MiB protocol limit'
              : 'protected scanner report exceeds 512 MiB protocol limit',
          )
          return
        }
        if (newline === -1) break
        const complete = parts.join('')
        discardPending()
        line(complete)
        if (failure !== null || ignoreOutput) return
        cursor = newline + 1
      }
    })
    child.stderr.on('data', (chunk: Buffer) => {
      stderr = Buffer.concat([stderr, chunk]).subarray(-MAX_STDERR_BYTES)
    })
    child.on('error', (error) => {
      failure ??= new Error(`protected scanner could not start: ${error.message}`)
    })
    child.on('close', (code, signal) => {
      if (killTimer !== undefined) clearTimeout(killTimer)
      if (!ignoreOutput && parts.length > 0) line(parts.join(''))
      discardPending()
      ancestors.length = 0
      if (failure !== null) {
        discardTree()
        rejectDone(failure)
      } else if (code !== 0 || signal !== null) {
        discardTree()
        rejectDone(new Error(`protected scanner exited (${signal ?? code}): ${stderr.toString('utf8').trim()}`))
      } else if (report === null) {
        discardTree()
        rejectDone(new Error('protected scanner exited without a final report'))
      } else {
        const accepted: LocalScanResult = { report, index: streamed ? index : null }
        if (streamed) index.seal()
        index = new ScanIndex()
        report = null
        resolveDone(accepted)
      }
    })
  })
  return { child, done, terminate }
}
