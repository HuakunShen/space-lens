import test from "ava";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import ts from "typescript";

const repoRoot = join(fileURLToPath(import.meta.url), "..", "..");

async function loadBenchmarkCli(t) {
  const sourcePath = join(repoRoot, "scripts", "benchmark-cli.ts");
  const outputPath = join(
    repoRoot,
    "scripts",
    `.benchmark-cli.${process.pid}.${Date.now()}.mjs`,
  );
  const source = readFileSync(sourcePath, "utf8");
  const output = ts.transpileModule(source, {
    compilerOptions: {
      target: ts.ScriptTarget.ES2023,
      module: ts.ModuleKind.ES2022,
      moduleResolution: ts.ModuleResolutionKind.Bundler,
      esModuleInterop: true,
    },
  }).outputText;

  writeFileSync(outputPath, output);
  t.teardown(() => rmSync(outputPath, { force: true }));

  return import(`${pathToFileURL(outputPath).href}?t=${Date.now()}`);
}

function createMockScanners() {
  const dustTree = [
    {
      name: "root",
      size: 4096,
      depth: 0,
      children: [{ name: "dust.bin", size: 4096, depth: 0, children: [] }],
    },
  ];
  const compactTree = [
    {
      name: "root",
      size: 4096,
      depth: 0,
      ignored: false,
      collapsed: false,
      children: [{ name: "compact.bin", size: 4096, depth: 0, ignored: false, collapsed: false, children: [] }],
    },
  ];

  return {
    buildDirectoryTree() {
      return dustTree;
    },
    scanCompact() {
      return compactTree;
    },
    getLargestNodes() {
      return { name: "root", size: 4096, children: [] };
    },
  };
}

test("runBenchmark exports the selected compact tree", async (t) => {
  const { runBenchmark } = await loadBenchmarkCli(t);
  const root = mkdtempSync(join(tmpdir(), "space-lens-cli-"));
  t.teardown(() => rmSync(root, { recursive: true, force: true }));
  const exportPath = join(root, "tree.json");
  const logs = [];

  await runBenchmark(
    {
      dir: root,
      top: "5",
      mode: "compact",
      exportTree: exportPath,
      jsonSize: true,
      ignoreHidden: false,
      fullPath: false,
      respectGitignore: true,
      ignoredMode: "summarize",
    },
    {
      scanners: createMockScanners(),
      log: (line = "") => logs.push(line),
    },
  );

  const exported = JSON.parse(readFileSync(exportPath, "utf8"));
  t.true(Array.isArray(exported));
  t.is(exported[0].children[0].name, "compact.bin");
  t.true(logs.some((line) => line.includes("exported tree:")));
});

test("runBenchmark can compare both scanners without exporting", async (t) => {
  const { runBenchmark } = await loadBenchmarkCli(t);
  const root = mkdtempSync(join(tmpdir(), "space-lens-cli-"));
  t.teardown(() => rmSync(root, { recursive: true, force: true }));
  const logs = [];

  await runBenchmark(
    {
      dir: root,
      top: "3",
      mode: "both",
      jsonSize: true,
      ignoreHidden: false,
      fullPath: false,
      respectGitignore: true,
      ignoredMode: "summarize",
    },
    {
      scanners: createMockScanners(),
      log: (line = "") => logs.push(line),
    },
  );

  t.true(logs.includes("size comparison"));
  t.true(logs.some((line) => line.includes("compact - dust:")));
  t.true(logs.some((line) => line.includes("Top 3 from dust full tree:")));
});
