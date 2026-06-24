/**
 * Copies the shared Space Lens Svelte SPA into the Kunkun extension dist folder.
 * The plugin package owns only packaging/backend glue; UI source remains in
 * apps/web so standalone and Kunkun modes render the same application. Bun runs
 * this TypeScript script directly during plugin packaging.
 */
import { cp, mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

interface ScannerPackageJson {
  readonly name: string;
  readonly version: string;
  readonly napi: {
    readonly binaryName: string;
    readonly targets: readonly string[];
  };
}

interface CopyNodeArtifactsOptions {
  readonly allowedArtifacts?: ReadonlySet<string>;
  readonly skipDirectories?: readonly string[];
}

const here = dirname(fileURLToPath(import.meta.url));
const pluginRoot = resolve(here, "..");
const repoRoot = resolve(pluginRoot, "../..");
const webBuild = resolve(repoRoot, "apps/web/build");
const scannerPackage = resolve(repoRoot, "packages/node");
const dist = resolve(pluginRoot, "dist");
const scannerOnly = process.argv.includes("--scanner-only");

await mkdir(dist, { recursive: true });
if (!scannerOnly) {
  await cp(webBuild, dist, { recursive: true });
  await rewriteIndexForCustomViewRoute(resolve(dist, "index.html"));
}
await copyScannerRuntime();

async function rewriteIndexForCustomViewRoute(indexPath: string): Promise<void> {
  const html = await readFile(indexPath, "utf8");
  const rewritten = html
    .replaceAll('href="/_app/', 'href="./_app/')
    .replaceAll('src="/_app/', 'src="./_app/')
    .replaceAll('import("/_app/', 'import("./_app/');

  await writeFile(indexPath, rewritten);
}

async function copyScannerRuntime(): Promise<void> {
  const scannerDist = resolve(dist, "node_modules/space-lens");
  const scannerPackageJson = parseScannerPackageJson(
    await readFile(resolve(scannerPackage, "package.json"), "utf8"),
  );
  const expectedArtifacts = scannerPackageJson.napi.targets
    .map((target) => napiTargetToArtifact(scannerPackageJson.napi.binaryName, target))
    .filter((artifact): artifact is string => artifact !== null)
    .sort();
  const allowedArtifacts = new Set(expectedArtifacts);
  await rm(scannerDist, { recursive: true, force: true });
  await mkdir(scannerDist, { recursive: true });

  await Promise.all([
    cp(resolve(scannerPackage, "index.js"), resolve(scannerDist, "index.js")),
    cp(resolve(scannerPackage, "index.d.ts"), resolve(scannerDist, "index.d.ts")),
    cp(resolve(scannerPackage, "package.json"), resolve(scannerDist, "package.json")),
  ]);

  const artifacts = new Set([
    ...(await copyNodeArtifacts(resolve(scannerPackage, "npm"), scannerDist, { allowedArtifacts })),
    ...(await copyNodeArtifacts(scannerPackage, scannerDist, { allowedArtifacts, skipDirectories: ["npm"] })),
  ]);
  const report = createScannerRuntimeReport(scannerPackageJson, expectedArtifacts, [...artifacts].sort());
  await writeFile(resolve(dist, "scanner-runtime-report.json"), `${JSON.stringify(report, null, 2)}\n`);
  if (process.env.SPACE_LENS_REQUIRE_ALL_NATIVE_ARTIFACTS === "1" && !report.crossPlatformComplete) {
    throw new Error(`Missing Space Lens native scanner artifacts: ${report.missingArtifacts.join(", ")}`);
  }
}

async function copyNodeArtifacts(
  sourceDir: string,
  targetDir: string,
  options: CopyNodeArtifactsOptions = {},
): Promise<string[]> {
  const copied: string[] = [];
  const entries = await readdir(sourceDir, { withFileTypes: true }).catch(() => []);
  for (const entry of entries) {
    const source = resolve(sourceDir, entry.name);
    if (entry.isDirectory()) {
      if (options.skipDirectories?.includes(entry.name)) continue;
      copied.push(...(await copyNodeArtifacts(source, targetDir, options)));
      continue;
    }
    if (entry.isFile() && entry.name.endsWith(".node")) {
      if (options.allowedArtifacts && !options.allowedArtifacts.has(entry.name)) continue;
      await cp(source, resolve(targetDir, entry.name));
      copied.push(entry.name);
    }
  }
  return copied;
}

function createScannerRuntimeReport(
  scannerPackageJson: ScannerPackageJson,
  expectedArtifacts: readonly string[],
  copiedArtifacts: readonly string[],
) {
  const copied = [...new Set(copiedArtifacts)].sort();
  const missingArtifacts = expectedArtifacts.filter((artifact) => !copied.includes(artifact));
  const currentArtifact = currentPlatformArtifact(scannerPackageJson.napi.binaryName);

  return {
    packageName: scannerPackageJson.name,
    packageVersion: scannerPackageJson.version,
    currentPlatform: {
      platform: process.platform,
      arch: process.arch,
      artifact: currentArtifact,
      bundled: copied.includes(currentArtifact),
    },
    expectedArtifacts,
    bundledArtifacts: copied,
    missingArtifacts,
    crossPlatformComplete: missingArtifacts.length === 0,
    releaseHint:
      missingArtifacts.length === 0
        ? "All configured NAPI targets are bundled."
        : "Local dev builds may bundle only the current platform. Set SPACE_LENS_REQUIRE_ALL_NATIVE_ARTIFACTS=1 in release CI to fail when target artifacts are missing.",
  };
}

function napiTargetToArtifact(binaryName: string, target: string): string | null {
  const targetToArtifact: Record<string, string> = {
    "x86_64-pc-windows-msvc": `${binaryName}.win32-x64-msvc.node`,
    "aarch64-pc-windows-msvc": `${binaryName}.win32-arm64-msvc.node`,
    "x86_64-apple-darwin": `${binaryName}.darwin-x64.node`,
    "aarch64-apple-darwin": `${binaryName}.darwin-arm64.node`,
    "x86_64-unknown-linux-gnu": `${binaryName}.linux-x64-gnu.node`,
    "x86_64-unknown-linux-musl": `${binaryName}.linux-x64-musl.node`,
    "aarch64-unknown-linux-gnu": `${binaryName}.linux-arm64-gnu.node`,
    "aarch64-unknown-linux-musl": `${binaryName}.linux-arm64-musl.node`,
  };
  return targetToArtifact[target] ?? null;
}

function currentPlatformArtifact(binaryName: string): string {
  const arch = process.arch === "x64" ? "x64" : process.arch;
  if (process.platform === "win32") {
    return `${binaryName}.win32-${arch}-msvc.node`;
  }
  if (process.platform === "darwin") {
    return `${binaryName}.darwin-${arch}.node`;
  }
  if (process.platform === "linux") {
    return `${binaryName}.linux-${arch}-${isMuslRuntime() ? "musl" : "gnu"}.node`;
  }
  return `${binaryName}.${process.platform}-${arch}.node`;
}

function isMuslRuntime(): boolean {
  if (process.platform !== "linux") return false;
  const report = process.report?.getReport?.() as
    | { readonly header?: { readonly glibcVersionRuntime?: string } }
    | undefined;
  return !report?.header?.glibcVersionRuntime;
}

function parseScannerPackageJson(source: string): ScannerPackageJson {
  const value: unknown = JSON.parse(source);
  if (!isScannerPackageJson(value)) {
    throw new Error("Invalid space-lens scanner package.json: missing napi binary metadata.");
  }
  return value;
}

function isScannerPackageJson(value: unknown): value is ScannerPackageJson {
  if (typeof value !== "object" || value === null) return false;
  const packageJson = value as {
    readonly name?: unknown;
    readonly version?: unknown;
    readonly napi?: {
      readonly binaryName?: unknown;
      readonly targets?: unknown;
    };
  };
  return (
    typeof packageJson.name === "string" &&
    typeof packageJson.version === "string" &&
    typeof packageJson.napi?.binaryName === "string" &&
    Array.isArray(packageJson.napi.targets) &&
    packageJson.napi.targets.every((target) => typeof target === "string")
  );
}
