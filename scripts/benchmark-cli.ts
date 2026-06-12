import { existsSync, readFileSync } from "node:fs";
import { mkdir, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, isAbsolute, resolve, sep } from "node:path";
import { defineCommand } from "citty";

import { buildDirectoryTree, getLargestNodes, scanCompact } from "../index.js";
import type {
  CompactNode,
  CompactScanOptions,
  DirectoryTreeOptions,
  DisplayNode,
  Node as DirectoryNode,
} from "../index.js";

const MODES = ["both", "dust", "compact"] as const;
const IGNORED_MODES = ["summarize", "exclude"] as const;
const PACKAGE_VERSION = JSON.parse(
  readFileSync(new URL("../package.json", import.meta.url), "utf8"),
).version as string;

type Mode = (typeof MODES)[number];
type IgnoredMode = (typeof IGNORED_MODES)[number];
type Log = (line?: string) => void;
type RawArgs = Record<string, unknown>;

type TreeNode = {
  size: number;
  children: TreeNode[];
};

type Scanners = {
  buildDirectoryTree(options: DirectoryTreeOptions): DirectoryNode[];
  getLargestNodes(nodes: DirectoryNode[], numberOfNodes: number): DisplayNode | null;
  scanCompact(options: CompactScanOptions): CompactNode[];
};

type BenchmarkOptions = {
  scanners?: Scanners;
  log?: Log;
};

type NormalizedArgs = {
  dir: string;
  mode: Mode;
  top: number;
  exportTree?: string;
  jsonSize: boolean;
  ignoreHidden: boolean;
  fullPath: boolean;
  respectGitignore: boolean;
  ignoredMode: IgnoredMode;
};

type MemorySnapshot = {
  rss: number;
  heapUsed: number;
  external: number;
  peakRss: number | undefined;
};

type DustResult = {
  tree: DirectoryNode[];
  largestNodes: DisplayNode | null;
  buildSeconds: string;
  largestSeconds: string;
  totalSeconds: string;
  totalSize: number;
  nodes: number;
  jsonBytes: number | undefined;
  memory: MemorySnapshot;
};

type CompactResult = {
  tree: CompactNode[];
  seconds: string;
  totalSize: number;
  nodes: number;
  jsonBytes: number | undefined;
  memory: MemorySnapshot;
};

type ExportedTree = {
  path: string;
  bytes: number;
};

type BenchmarkResult = {
  args: NormalizedArgs;
  dust: DustResult | undefined;
  compact: CompactResult | undefined;
  exported: ExportedTree | undefined;
};

type ExportTrees = {
  dust?: DirectoryNode[];
  compact?: CompactNode[];
};

const defaultScanners: Scanners = {
  buildDirectoryTree,
  getLargestNodes,
  scanCompact,
};

export function createBenchmarkCommand(options: BenchmarkOptions = {}) {
  return defineCommand({
    meta: {
      name: "space-lens-bench",
      description: "Benchmark space-lens directory scanners and optionally export trees.",
      version: PACKAGE_VERSION,
    },
    args: {
      dir: {
        type: "positional",
        description: "Directory to scan.",
        required: false,
        default: process.cwd(),
        valueHint: "DIR",
      },
      mode: {
        type: "enum",
        description: "Scanner mode to run.",
        options: [...MODES],
        default: "both",
        alias: ["m"],
      },
      top: {
        type: "string",
        description: "Number of largest dust nodes to print.",
        default: "10",
        alias: ["t"],
        valueHint: "N",
      },
      "export-tree": {
        type: "string",
        description: "Write the scanned tree JSON to this path.",
        alias: ["o"],
        valueHint: "PATH",
      },
      "json-size": {
        type: "boolean",
        description: "Measure JSON payload size.",
        negativeDescription: "Skip JSON payload size measurement.",
        default: true,
      },
      "ignore-hidden": {
        type: "boolean",
        description: "Skip dotfiles and dot directories.",
        default: false,
      },
      "full-path": {
        type: "boolean",
        description: "Store full paths in tree nodes.",
        default: false,
      },
      "respect-gitignore": {
        type: "boolean",
        description: "Respect .gitignore files in compact mode.",
        negativeDescription: "Ignore .gitignore files in compact mode.",
        default: true,
      },
      "ignored-mode": {
        type: "enum",
        description: "How compact mode handles gitignored directories.",
        options: [...IGNORED_MODES],
        default: "summarize",
      },
    },
    async run({ args }) {
      await runBenchmark(args, options);
    },
  });
}

export async function runBenchmark(
  rawArgs: RawArgs,
  options: BenchmarkOptions = {},
): Promise<BenchmarkResult> {
  const log = options.log ?? console.log;
  const scanners = options.scanners ?? defaultScanners;
  const args = normalizeArgs(rawArgs);

  if (!existsSync(args.dir)) {
    throw new Error(`Directory does not exist: ${args.dir}`);
  }

  let dustResult;
  let compactResult;

  if (args.mode === "both" || args.mode === "dust") {
    dustResult = runDust(args, scanners);
  }

  if (args.mode === "both" || args.mode === "compact") {
    compactResult = runCompact(args, scanners);
  }

  log(`Directory: ${args.dir}`);
  log(`Mode:      ${args.mode}`);
  log("");

  if (dustResult) {
    printDustResult(dustResult, args, log);
  }

  if (compactResult) {
    printCompactResult(compactResult, args, log);
  }

  if (dustResult && compactResult) {
    printSizeComparison(dustResult.totalSize, compactResult.totalSize, log);
  }

  if (dustResult?.largestNodes) {
    log(`Top ${args.top} from dust full tree:`);
    log(formatObject(dustResult.largestNodes));
  }

  let exported;
  if (args.exportTree) {
    exported = await exportTree(args, { dust: dustResult?.tree, compact: compactResult?.tree });
    log(`exported tree:      ${exported.path} (${formatBytes(exported.bytes)}, ${exported.bytes} bytes)`);
  }

  return {
    args,
    dust: dustResult,
    compact: compactResult,
    exported,
  };
}

function normalizeArgs(rawArgs: RawArgs): NormalizedArgs {
  const mode = String(rawArgs.mode ?? "both");
  if (!isMode(mode)) {
    throw new Error(`Unknown mode "${mode}". Use ${MODES.join(", ")}.`);
  }

  const ignoredMode = String(getArg(rawArgs, "ignoredMode", "ignored-mode") ?? "summarize");
  if (!isIgnoredMode(ignoredMode)) {
    throw new Error(`Unknown ignoredMode "${ignoredMode}". Use ${IGNORED_MODES.join(", ")}.`);
  }

  const top = Number(rawArgs.top ?? 10);
  if (!Number.isInteger(top) || top < 1) {
    throw new Error("--top must be a positive integer");
  }

  return {
    dir: expandPath(String(rawArgs.dir ?? process.cwd())),
    mode,
    top,
    exportTree: getArg(rawArgs, "exportTree", "export-tree")
      ? expandPath(String(getArg(rawArgs, "exportTree", "export-tree")))
      : undefined,
    jsonSize: getArg(rawArgs, "jsonSize", "json-size") !== false,
    ignoreHidden: Boolean(getArg(rawArgs, "ignoreHidden", "ignore-hidden")),
    fullPath: Boolean(getArg(rawArgs, "fullPath", "full-path")),
    respectGitignore: getArg(rawArgs, "respectGitignore", "respect-gitignore") !== false,
    ignoredMode,
  };
}

function isMode(value: string): value is Mode {
  return MODES.includes(value as Mode);
}

function isIgnoredMode(value: string): value is IgnoredMode {
  return IGNORED_MODES.includes(value as IgnoredMode);
}

function getArg(args: RawArgs, camelName: string, kebabName: string): unknown {
  return args[camelName] ?? args[kebabName];
}

function runDust(args: NormalizedArgs, scanners: Scanners): DustResult {
  const start = performance.now();
  const tree = scanners.buildDirectoryTree({
    directories: [args.dir],
    ignoreHidden: args.ignoreHidden,
    fullPath: args.fullPath,
  });
  const afterTree = performance.now();

  const largestNodes = scanners.getLargestNodes(tree, args.top);
  const end = performance.now();

  return {
    tree,
    largestNodes,
    buildSeconds: seconds(afterTree - start),
    largestSeconds: seconds(end - afterTree),
    totalSeconds: seconds(end - start),
    totalSize: totalSize(tree),
    nodes: countNodes(tree),
    jsonBytes: args.jsonSize ? jsonSize(tree) : undefined,
    memory: memorySnapshot(),
  };
}

function runCompact(args: NormalizedArgs, scanners: Scanners): CompactResult {
  const start = performance.now();
  const tree = scanners.scanCompact({
    directories: [args.dir],
    ignoreHidden: args.ignoreHidden,
    fullPath: args.fullPath,
    respectGitignore: args.respectGitignore,
    ignoredMode: args.ignoredMode,
  });
  const end = performance.now();

  return {
    tree,
    seconds: seconds(end - start),
    totalSize: totalSize(tree),
    nodes: countNodes(tree),
    jsonBytes: args.jsonSize ? jsonSize(tree) : undefined,
    memory: memorySnapshot(),
  };
}

function printDustResult(result: DustResult, args: NormalizedArgs, log: Log): void {
  log("dust full tree");
  log(`  buildDirectoryTree: ${result.buildSeconds}`);
  log(`  getLargestNodes:    ${result.largestSeconds}`);
  log(`  total:              ${result.totalSeconds}`);
  log(`  total size:         ${formatBytes(result.totalSize)} (${result.totalSize} bytes)`);
  log(`  nodes:              ${result.nodes}`);
  printJsonSize(result.jsonBytes, log);
  printMemory(result.memory, args.mode === "both" ? "process peak after dust phase" : "single-method process peak", log);
  log("");
}

function printCompactResult(result: CompactResult, args: NormalizedArgs, log: Log): void {
  log("compact scanner");
  log(`  scanCompact:        ${result.seconds}`);
  log(`  total size:         ${formatBytes(result.totalSize)} (${result.totalSize} bytes)`);
  log(`  nodes:              ${result.nodes}`);
  printJsonSize(result.jsonBytes, log);
  printMemory(result.memory, args.mode === "both" ? "process peak after both phases" : "single-method process peak", log);
  log("");
}

function printJsonSize(jsonBytes: number | undefined, log: Log): void {
  if (jsonBytes === undefined) {
    log("  JSON size:          skipped");
    return;
  }

  log(`  JSON size:          ${formatBytes(jsonBytes)}`);
}

function printMemory(memory: MemorySnapshot, note: string, log: Log): void {
  const maxRssText = memory.peakRss === undefined ? "unavailable" : formatBytes(memory.peakRss);
  log(`  memory:            rss=${formatBytes(memory.rss)}, heapUsed=${formatBytes(memory.heapUsed)}, external=${formatBytes(memory.external)}`);
  log(`  peak RSS:          ${maxRssText}`);
  log(`  note:              ${note}`);
}

function printSizeComparison(
  dustTotalSize: number,
  compactTotalSize: number,
  log: Log,
): void {
  const sizeDiff = compactTotalSize - dustTotalSize;
  const sizeDiffPercent = dustTotalSize === 0 ? 0 : (sizeDiff / dustTotalSize) * 100;

  log("size comparison");
  log(`  compact - dust:     ${formatBytes(Math.abs(sizeDiff))} (${sizeDiff} bytes, ${sizeDiffPercent.toFixed(4)}%)`);
  log("");
}

async function exportTree(args: NormalizedArgs, trees: ExportTrees): Promise<ExportedTree> {
  if (!args.exportTree) {
    throw new Error("Missing export path");
  }

  const payload =
    args.mode === "dust"
      ? trees.dust
      : args.mode === "compact"
        ? trees.compact
        : { dust: trees.dust, compact: trees.compact };

  const json = `${JSON.stringify(payload)}\n`;
  await mkdir(dirname(args.exportTree), { recursive: true });
  await writeFile(args.exportTree, json, "utf8");

  return {
    path: args.exportTree,
    bytes: Buffer.byteLength(json),
  };
}

function expandPath(path: string): string {
  if (path === "~") {
    return homedir();
  }

  if (path.startsWith(`~${sep}`) || path.startsWith("~/")) {
    return resolve(homedir(), path.slice(2));
  }

  return isAbsolute(path) ? path : resolve(path);
}

function countNodes(nodes: TreeNode[]): number {
  let count = 0;
  const stack = [...nodes];

  while (stack.length > 0) {
    const node = stack.pop();
    if (!node) {
      continue;
    }

    count += 1;
    stack.push(...node.children);
  }

  return count;
}

function totalSize(nodes: TreeNode[]): number {
  return nodes.reduce((sum, node) => sum + node.size, 0);
}

function jsonSize(value: unknown): number {
  return Buffer.byteLength(JSON.stringify(value));
}

function memorySnapshot(): MemorySnapshot {
  const memory = process.memoryUsage();

  return {
    rss: memory.rss,
    heapUsed: memory.heapUsed,
    external: memory.external,
    peakRss: maxResidentSetSize(),
  };
}

function maxResidentSetSize(): number | undefined {
  if (typeof process.resourceUsage !== "function") {
    return undefined;
  }

  const maxRss = process.resourceUsage().maxRSS;
  const versions = process.versions as NodeJS.ProcessVersions & { bun?: string };
  return versions.bun ? maxRss : maxRss * 1024;
}

function formatBytes(bytes: number): string {
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let value = bytes;
  let unitIndex = 0;

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

function seconds(ms: number): string {
  return `${(ms / 1000).toFixed(3)}s`;
}

function formatObject(value: unknown): string {
  return JSON.stringify(value, null, 2);
}
