const { existsSync } = require("node:fs");
const { homedir } = require("node:os");
const { isAbsolute, resolve, sep } = require("node:path");
const { performance } = require("node:perf_hooks");
const { buildDirectoryTree, getLargestNodes } = require("../index.js");

type DisplayNode = import("../index.js").DisplayNode;
type Node = import("../index.js").Node;

const DEFAULT_HIDDEN_NAMES = [
  "node_modules",
  "target",
  "dist",
  "build",
  ".git",
  ".next",
  ".turbo",
  ".cache",
];

type Args = {
  dir: string;
  top: number;
  ignoreHidden: boolean;
  hiddenNames: Set<string>;
};

function parseArgs(argv: string[]): Args {
  let dir = process.cwd();
  let top = 30;
  let ignoreHidden = true;
  let hiddenNames = new Set(DEFAULT_HIDDEN_NAMES);

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];

    if (arg === "--dir") {
      dir = requiredValue(argv, i);
      i += 1;
    } else if (arg === "--top") {
      top = Number(requiredValue(argv, i));
      i += 1;
    } else if (arg === "--ignore-hidden") {
      ignoreHidden = true;
    } else if (arg === "--include-hidden") {
      ignoreHidden = false;
    } else if (arg === "--hide") {
      hiddenNames = new Set(
        requiredValue(argv, i)
          .split(",")
          .map((name) => name.trim())
          .filter(Boolean),
      );
      i += 1;
    } else if (arg === "--help" || arg === "-h") {
      printUsage();
      process.exit(0);
    } else if (!arg.startsWith("-")) {
      dir = arg;
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }

  if (!Number.isInteger(top) || top < 1) {
    throw new Error("--top must be a positive integer");
  }

  return {
    dir: expandPath(dir),
    top,
    ignoreHidden,
    hiddenNames,
  };
}

function requiredValue(argv: string[], index: number): string {
  const value = argv[index + 1];
  if (!value || value.startsWith("-")) {
    throw new Error(`Missing value for ${argv[index]}`);
  }
  return value;
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

function formatSeconds(ms: number): string {
  return `${(ms / 1000).toFixed(3)}s`;
}

function basename(path: string): string {
  const normalized = path.replaceAll("\\", "/");
  const parts = normalized.split("/").filter(Boolean);
  return parts.at(-1) ?? path;
}

function hideDisplayDetails(node: DisplayNode, hiddenNames: Set<string>): DisplayNode {
  const name = basename(node.name);
  if (hiddenNames.has(name)) {
    return {
      ...node,
      name: `${node.name} [details hidden]`,
      children: [],
    };
  }

  return {
    ...node,
    children: node.children.map((child) => hideDisplayDetails(child, hiddenNames)),
  };
}

function countNodes(nodes: Node[]): number {
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

function printNode(node: DisplayNode, indent = ""): void {
  console.log(`${indent}${formatBytes(node.size)}  ${node.name}`);

  for (const child of node.children) {
    printNode(child, `${indent}  `);
  }
}

function printUsage(): void {
  console.log(`Usage:
  npx tsx scripts/benchmark.ts ~/Dev/CrossCopy
  npx tsx scripts/benchmark.ts --dir ~/Dev/CrossCopy --top 20
  npx tsx scripts/benchmark.ts ~/Dev/CrossCopy --hide node_modules,target,dist

Options:
  --dir <path>          Directory to scan. Positional path also works.
  --top <number>       Number of largest nodes to display. Default: 30.
  --ignore-hidden      Skip dotfiles/dot directories during scan. Default.
  --include-hidden     Include dotfiles/dot directories during scan.
  --hide <names>       Comma-separated names to collapse in printed output.

Note:
  --hide only collapses details after scanning. The current native API still
  scans those directories to calculate their sizes.`);
}

async function main(): Promise<void> {
  const args = parseArgs(process.argv.slice(2));

  if (!existsSync(args.dir)) {
    throw new Error(`Directory does not exist: ${args.dir}`);
  }

  console.log(`Directory: ${args.dir}`);
  console.log(`Top nodes: ${args.top}`);
  console.log(`Ignore hidden during scan: ${args.ignoreHidden}`);
  console.log(`Collapse names in output: ${[...args.hiddenNames].join(", ")}`);
  console.log("");

  const treeStart = performance.now();
  const tree = buildDirectoryTree({
    directories: [args.dir],
    ignoreHidden: args.ignoreHidden,
    fullPath: true,
  });
  const treeEnd = performance.now();

  const largestStart = performance.now();
  const largest = getLargestNodes(tree, args.top);
  const largestEnd = performance.now();

  console.log(`buildDirectoryTree: ${formatSeconds(treeEnd - treeStart)}`);
  console.log(`getLargestNodes:   ${formatSeconds(largestEnd - largestStart)}`);
  console.log(`total:             ${formatSeconds(largestEnd - treeStart)}`);
  console.log(`indexed nodes:     ${countNodes(tree)}`);

  if (largest) {
    console.log("");
    printNode(hideDisplayDetails(largest, args.hiddenNames));
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
});
