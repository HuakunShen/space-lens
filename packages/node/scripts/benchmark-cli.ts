import { existsSync, readFileSync } from 'node:fs'
import { mkdir, writeFile } from 'node:fs/promises'
import { homedir } from 'node:os'
import { dirname, isAbsolute, resolve, sep } from 'node:path'
import { defineCommand } from 'citty'

import { scanDirectory } from '../index.js'
import type { DirectoryNode, DirectoryScanOptions } from '../index.js'

const IGNORED_MODES = ['summarize', 'exclude'] as const
const PACKAGE_VERSION = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')).version as string

type IgnoredMode = (typeof IGNORED_MODES)[number]
type Log = (line?: string) => void
type RawArgs = Record<string, unknown>

type TreeNode = {
  size: number
  children: TreeNode[]
}

type Scanners = {
  scanDirectory(options: DirectoryScanOptions): DirectoryNode[]
}

type BenchmarkOptions = {
  scanners?: Scanners
  log?: Log
}

type NormalizedArgs = {
  dir: string
  exportTree?: string
  jsonSize: boolean
  ignoreHidden: boolean
  fullPath: boolean
  respectGitignore: boolean
  ignoredMode: IgnoredMode
}

type MemorySnapshot = {
  rss: number
  heapUsed: number
  external: number
  peakRss: number | undefined
}

type DirectoryResult = {
  tree: DirectoryNode[]
  seconds: string
  totalSize: number
  nodes: number
  jsonBytes: number | undefined
  memory: MemorySnapshot
}

type ExportedTree = {
  path: string
  bytes: number
}

type BenchmarkResult = {
  args: NormalizedArgs
  directory: DirectoryResult
  exported: ExportedTree | undefined
}

const defaultScanners: Scanners = {
  scanDirectory,
}

export function createBenchmarkCommand(options: BenchmarkOptions = {}) {
  return defineCommand({
    meta: {
      name: 'space-lens-bench',
      description: 'Benchmark the space-lens directory scanner.',
      version: PACKAGE_VERSION,
    },
    args: {
      dir: {
        type: 'positional',
        description: 'Directory to scan.',
        required: false,
        default: process.cwd(),
        valueHint: 'DIR',
      },
      'export-tree': {
        type: 'string',
        description: 'Write the scanned tree JSON to this path.',
        alias: ['o'],
        valueHint: 'PATH',
      },
      'json-size': {
        type: 'boolean',
        description: 'Measure JSON payload size.',
        negativeDescription: 'Skip JSON payload size measurement.',
        default: true,
      },
      'ignore-hidden': {
        type: 'boolean',
        description: 'Skip dotfiles and dot directories.',
        default: false,
      },
      'full-path': {
        type: 'boolean',
        description: 'Store full paths in tree nodes.',
        default: false,
      },
      'respect-gitignore': {
        type: 'boolean',
        description: 'Respect .gitignore files.',
        negativeDescription: 'Ignore .gitignore files.',
        default: true,
      },
      'ignored-mode': {
        type: 'enum',
        description: 'How gitignored directories are handled.',
        options: [...IGNORED_MODES],
        default: 'summarize',
      },
    },
    async run({ args }) {
      await runBenchmark(args, options)
    },
  })
}

export async function runBenchmark(rawArgs: RawArgs, options: BenchmarkOptions = {}): Promise<BenchmarkResult> {
  const log = options.log ?? console.log
  const scanners = options.scanners ?? defaultScanners
  const args = normalizeArgs(rawArgs)

  if (!existsSync(args.dir)) {
    throw new Error(`Directory does not exist: ${args.dir}`)
  }

  const directory = runDirectoryScan(args, scanners)

  log(`Directory: ${args.dir}`)
  log('')
  printDirectoryResult(directory, log)

  let exported
  if (args.exportTree) {
    exported = await exportTree(args, directory.tree)
    log(`exported tree:      ${exported.path} (${formatBytes(exported.bytes)}, ${exported.bytes} bytes)`)
  }

  return {
    args,
    directory,
    exported,
  }
}

function normalizeArgs(rawArgs: RawArgs): NormalizedArgs {
  const ignoredMode = String(getArg(rawArgs, 'ignoredMode', 'ignored-mode') ?? 'summarize')
  if (!isIgnoredMode(ignoredMode)) {
    throw new Error(`Unknown ignoredMode "${ignoredMode}". Use ${IGNORED_MODES.join(', ')}.`)
  }

  return {
    dir: expandPath(String(rawArgs.dir ?? process.cwd())),
    exportTree: getArg(rawArgs, 'exportTree', 'export-tree')
      ? expandPath(String(getArg(rawArgs, 'exportTree', 'export-tree')))
      : undefined,
    jsonSize: getArg(rawArgs, 'jsonSize', 'json-size') !== false,
    ignoreHidden: Boolean(getArg(rawArgs, 'ignoreHidden', 'ignore-hidden')),
    fullPath: Boolean(getArg(rawArgs, 'fullPath', 'full-path')),
    respectGitignore: getArg(rawArgs, 'respectGitignore', 'respect-gitignore') !== false,
    ignoredMode,
  }
}

function isIgnoredMode(value: string): value is IgnoredMode {
  return IGNORED_MODES.includes(value as IgnoredMode)
}

function getArg(args: RawArgs, camelName: string, kebabName: string): unknown {
  return args[camelName] ?? args[kebabName]
}

function runDirectoryScan(args: NormalizedArgs, scanners: Scanners): DirectoryResult {
  const start = performance.now()
  const tree = scanners.scanDirectory({
    directories: [args.dir],
    ignoreHidden: args.ignoreHidden,
    fullPath: args.fullPath,
    respectGitignore: args.respectGitignore,
    ignoredMode: args.ignoredMode,
  })
  const end = performance.now()

  return {
    tree,
    seconds: seconds(end - start),
    totalSize: totalSize(tree),
    nodes: countNodes(tree),
    jsonBytes: args.jsonSize ? jsonSize(tree) : undefined,
    memory: memorySnapshot(),
  }
}

function printDirectoryResult(result: DirectoryResult, log: Log): void {
  log('directory scanner')
  log(`  scanDirectory:      ${result.seconds}`)
  log(`  total size:         ${formatBytes(result.totalSize)} (${result.totalSize} bytes)`)
  log(`  nodes:              ${result.nodes}`)
  printJsonSize(result.jsonBytes, log)
  printMemory(result.memory, log)
  log('')
}

function printJsonSize(jsonBytes: number | undefined, log: Log): void {
  if (jsonBytes === undefined) {
    log('  JSON size:          skipped')
    return
  }

  log(`  JSON size:          ${formatBytes(jsonBytes)}`)
}

function printMemory(memory: MemorySnapshot, log: Log): void {
  const maxRssText = memory.peakRss === undefined ? 'unavailable' : formatBytes(memory.peakRss)
  log(
    `  memory:            rss=${formatBytes(memory.rss)}, heapUsed=${formatBytes(memory.heapUsed)}, external=${formatBytes(memory.external)}`,
  )
  log(`  peak RSS:          ${maxRssText}`)
}

async function exportTree(args: NormalizedArgs, tree: DirectoryNode[]): Promise<ExportedTree> {
  if (!args.exportTree) {
    throw new Error('Missing export path')
  }

  const json = `${JSON.stringify(tree)}\n`
  await mkdir(dirname(args.exportTree), { recursive: true })
  await writeFile(args.exportTree, json, 'utf8')

  return {
    path: args.exportTree,
    bytes: Buffer.byteLength(json),
  }
}

function expandPath(path: string): string {
  if (path === '~') {
    return homedir()
  }

  if (path.startsWith(`~${sep}`) || path.startsWith('~/')) {
    return resolve(homedir(), path.slice(2))
  }

  return isAbsolute(path) ? path : resolve(path)
}

function countNodes(nodes: TreeNode[]): number {
  let count = 0
  const stack = [...nodes]

  while (stack.length > 0) {
    const node = stack.pop()
    if (!node) {
      continue
    }

    count += 1
    stack.push(...node.children)
  }

  return count
}

function totalSize(nodes: TreeNode[]): number {
  return nodes.reduce((sum, node) => sum + node.size, 0)
}

function jsonSize(value: unknown): number {
  return Buffer.byteLength(JSON.stringify(value))
}

function memorySnapshot(): MemorySnapshot {
  const memory = process.memoryUsage()

  return {
    rss: memory.rss,
    heapUsed: memory.heapUsed,
    external: memory.external,
    peakRss: maxResidentSetSize(),
  }
}

function maxResidentSetSize(): number | undefined {
  if (typeof process.resourceUsage !== 'function') {
    return undefined
  }

  const maxRss = process.resourceUsage().maxRSS
  const versions = process.versions as NodeJS.ProcessVersions & { bun?: string }
  return versions.bun ? maxRss : maxRss * 1024
}

function formatBytes(bytes: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB']
  let value = bytes
  let unitIndex = 0

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024
    unitIndex += 1
  }

  return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`
}

function seconds(ms: number): string {
  return `${(ms / 1000).toFixed(3)}s`
}
