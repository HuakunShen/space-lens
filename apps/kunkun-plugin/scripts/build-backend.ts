/**
 * Builds the Space Lens Kunkun backend from either the Kunkun root workspace or
 * the nested Space Lens workspace. Bun runs this TypeScript script directly,
 * while tsdown bundles the backend into one JS file. The shared web package
 * does not declare @kunkunsh/sdk as a dependency because the SDK is unpublished
 * during local migration; this script links the local Kunkun checkout for
 * plugin builds. The bundle is checked so only Node builtins and the copied
 * native `space-lens` scanner package remain as runtime imports.
 */
import { $ } from "bun";
import { existsSync } from "node:fs";
import { lstat, mkdir, readFile, readlink, symlink, unlink } from "node:fs/promises";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const pluginRoot = resolve(here, "..");
const spaceLensRoot = resolve(pluginRoot, "../..");
const kunkunRoot = findKunkunRoot(pluginRoot);
const buildWeb = process.argv.includes("--build-web");
const scannerOnly = process.argv.includes("--scanner-only");
const copyWebAssets = process.argv.includes("--copy-web-assets");

process.chdir(pluginRoot);

if (!kunkunRoot) {
  throw new Error(
    "Cannot build the Kunkun backend: @kunkunsh/sdk is unpublished and this package is not inside a Kunkun checkout.",
  );
}

const kunkunWorkspaceRoot = kunkunRoot;
const kunkunSdkPackage = resolve(kunkunWorkspaceRoot, "packages/sdk");
const kunkunRequiredEntries = [
  resolve(kunkunSdkPackage, "manifest-schema.json"),
  resolve(kunkunSdkPackage, "dist/index.js"),
  resolve(kunkunSdkPackage, "dist/ui/custom.js"),
  resolve(kunkunSdkPackage, "dist/backend/index.js"),
];
const missingKunkunEntry = kunkunRequiredEntries.find((entry) => !existsSync(entry));
if (missingKunkunEntry) {
  throw new Error(
    `Cannot build the Kunkun plugin: ${missingKunkunEntry} is missing. Build @kunkunsh/sdk first from the Kunkun root.`,
  );
}

await ensureKunkunWorkspaceLinks();

if (buildWeb) {
  const previousKunkunBuild = process.env.SPACE_LENS_KUNKUN_BUILD;
  process.env.SPACE_LENS_KUNKUN_BUILD = "1";
  try {
    await $`pnpm --dir ${spaceLensRoot} --filter web build`;
  } finally {
    if (previousKunkunBuild === undefined) {
      delete process.env.SPACE_LENS_KUNKUN_BUILD;
    } else {
      process.env.SPACE_LENS_KUNKUN_BUILD = previousKunkunBuild;
    }
  }
}

await $`pnpm --dir ${spaceLensRoot} --filter @space-lens/cli build`;
await ensurePackageLink("@space-lens/cli", resolve(spaceLensRoot, "apps/tui"));
await ensurePackageLink("space-lens", resolve(spaceLensRoot, "packages/node"));

const tsdownBin = resolve(spaceLensRoot, "apps/tui/node_modules/.bin/tsdown");
if (!existsSync(tsdownBin)) {
  throw new Error(
    `Cannot build the Kunkun backend: ${tsdownBin} is missing. Run pnpm install from ${spaceLensRoot}.`,
  );
}

await $`${tsdownBin} --config tsdown.backend.config.ts`;
await assertSelfContainedBackendBundle(resolve(pluginRoot, "dist/backend.js"));

if (scannerOnly || copyWebAssets) {
  if (scannerOnly) {
    await $`bun scripts/copy-web-assets.ts --scanner-only`;
  } else {
    await $`bun scripts/copy-web-assets.ts`;
  }
}

function findKunkunRoot(startDir: string): string | null {
  let current = startDir;
  while (true) {
    if (existsSync(resolve(current, "packages/sdk/package.json"))) {
      return current;
    }
    const parent = dirname(current);
    if (parent === current) return null;
    current = parent;
  }
}

async function ensurePackageLink(packageName: string, targetPath: string): Promise<void> {
  await ensurePackageLinkAt(pluginRoot, packageName, targetPath);
}

async function ensureKunkunWorkspaceLinks(): Promise<void> {
  const observabilityPackage = resolve(kunkunWorkspaceRoot, "packages/observability");
  const kkrpcPackage = resolve(kunkunWorkspaceRoot, "vendors/kkrpc/packages/kkrpc");
  const webRoot = resolve(spaceLensRoot, "apps/web");

  await ensurePackageLink("@kunkunsh/sdk", kunkunSdkPackage);
  await ensurePackageLink("@kunkunsh/observability", observabilityPackage);
  await ensurePackageLink("kkrpc", kkrpcPackage);

  await ensurePackageLinkAt(webRoot, "@kunkunsh/sdk", kunkunSdkPackage);
  await ensurePackageLinkAt(kunkunWorkspaceRoot, "@kunkunsh/observability", observabilityPackage);
  await ensurePackageLinkAt(kunkunWorkspaceRoot, "kkrpc", kkrpcPackage);
}

async function ensurePackageLinkAt(root: string, packageName: string, targetPath: string): Promise<void> {
  const linkPath = resolve(root, "node_modules", ...packageName.split("/"));
  await mkdir(dirname(linkPath), { recursive: true });

  try {
    const stat = await lstat(linkPath);
    if (!stat.isSymbolicLink()) return;
    const existingTarget = resolve(dirname(linkPath), await readlink(linkPath));
    if (existingTarget === targetPath) return;
    await unlink(linkPath);
  } catch (error) {
    if (!isNodeError(error) || error.code !== "ENOENT") throw error;
  }

  await symlink(relative(dirname(linkPath), targetPath), linkPath, "dir");
}

async function assertSelfContainedBackendBundle(bundlePath: string): Promise<void> {
  const source = await readFile(bundlePath, "utf8");
  const unexpected = new Set<string>();
  for (const specifier of findBareImportSpecifiers(source)) {
    if (specifier.startsWith("node:")) continue;
    if (specifier === "space-lens") continue;
    unexpected.add(specifier);
  }
  if (unexpected.size === 0) return;
  throw new Error(
    `Backend bundle contains runtime package imports: ${[...unexpected].sort().join(", ")}. ` +
      "Bundle JS dependencies into dist/backend.js; only node:* and the copied native space-lens package may remain external.",
  );
}

function findBareImportSpecifiers(source: string): string[] {
  const specifiers: string[] = [];
  const patterns = [
    /\b(?:import|export)\s+(?:[^'"]*?\s+from\s*)?['"]([^.'"/][^'"]*)['"]/g,
    /\bimport\s*\(\s*['"]([^.'"/][^'"]*)['"]\s*\)/g,
  ];
  for (const pattern of patterns) {
    for (const match of source.matchAll(pattern)) {
      if (match[1]) specifiers.push(match[1]);
    }
  }
  return specifiers;
}

function isNodeError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error && "code" in error;
}
