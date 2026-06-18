/**
 * Copies the shared Space Lens Svelte SPA into the Kunkun extension dist folder.
 * The plugin package owns only packaging/backend glue; UI source remains in
 * apps/web so standalone and Kunkun modes render the same application.
 */
import { cp, mkdir, readdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const pluginRoot = resolve(here, '..')
const repoRoot = resolve(pluginRoot, '../..')
const webBuild = resolve(repoRoot, 'apps/web/build')
const scannerPackage = resolve(repoRoot, 'packages/node')
const dist = resolve(pluginRoot, 'dist')
const scannerOnly = process.argv.includes('--scanner-only')

await mkdir(dist, { recursive: true })
if (!scannerOnly) {
  await cp(webBuild, dist, { recursive: true })
  await rewriteIndexForCustomViewRoute(resolve(dist, 'index.html'))
}
await copyScannerRuntime()

async function rewriteIndexForCustomViewRoute(indexPath) {
  const html = await readFile(indexPath, 'utf8')
  const rewritten = html
    .replaceAll('href="/_app/', 'href="./_app/')
    .replaceAll('src="/_app/', 'src="./_app/')
    .replaceAll('import("/_app/', 'import("./_app/')

  await writeFile(indexPath, rewritten)
}

async function copyScannerRuntime() {
  const scannerDist = resolve(dist, 'node_modules/space-lens')
  await mkdir(scannerDist, { recursive: true })

  await Promise.all([
    cp(resolve(scannerPackage, 'index.js'), resolve(scannerDist, 'index.js')),
    cp(resolve(scannerPackage, 'index.d.ts'), resolve(scannerDist, 'index.d.ts')),
    cp(resolve(scannerPackage, 'package.json'), resolve(scannerDist, 'package.json')),
  ])

  const artifacts = new Set([
    ...(await copyNodeArtifacts(resolve(scannerPackage, 'npm'), scannerDist)),
    ...(await copyNodeArtifacts(scannerPackage, scannerDist, { skipDirectories: ['npm'] })),
  ])
  const report = await createScannerRuntimeReport([...artifacts].sort())
  await writeFile(resolve(dist, 'scanner-runtime-report.json'), `${JSON.stringify(report, null, 2)}\n`)
  if (process.env.SPACE_LENS_REQUIRE_ALL_NATIVE_ARTIFACTS === '1' && !report.crossPlatformComplete) {
    throw new Error(
      `Missing Space Lens native scanner artifacts: ${report.missingArtifacts.join(', ')}`,
    )
  }
}

async function copyNodeArtifacts(sourceDir, targetDir, options = {}) {
  const copied = []
  const entries = await readdir(sourceDir, { withFileTypes: true }).catch(() => [])
  for (const entry of entries) {
    const source = resolve(sourceDir, entry.name)
    if (entry.isDirectory()) {
      if (options.skipDirectories?.includes(entry.name)) continue
      copied.push(...(await copyNodeArtifacts(source, targetDir, options)))
      continue
    }
    if (entry.isFile() && entry.name.endsWith('.node')) {
      await cp(source, resolve(targetDir, entry.name))
      copied.push(entry.name)
    }
  }
  return copied
}

async function createScannerRuntimeReport(copiedArtifacts) {
  const scannerPackageJson = JSON.parse(await readFile(resolve(scannerPackage, 'package.json'), 'utf8'))
  const expectedArtifacts = scannerPackageJson.napi.targets
    .map((target) => napiTargetToArtifact(target))
    .filter(Boolean)
    .sort()
  const copied = [...new Set(copiedArtifacts)].sort()
  const missingArtifacts = expectedArtifacts.filter((artifact) => !copied.includes(artifact))
  const currentArtifact = currentPlatformArtifact()

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
        ? 'All configured NAPI targets are bundled.'
        : 'Local dev builds may bundle only the current platform. Set SPACE_LENS_REQUIRE_ALL_NATIVE_ARTIFACTS=1 in release CI to fail when target artifacts are missing.',
  }
}

function napiTargetToArtifact(target) {
  const targetToArtifact = {
    'x86_64-pc-windows-msvc': 'space-lens.win32-x64-msvc.node',
    'aarch64-pc-windows-msvc': 'space-lens.win32-arm64-msvc.node',
    'x86_64-apple-darwin': 'space-lens.darwin-x64.node',
    'aarch64-apple-darwin': 'space-lens.darwin-arm64.node',
    'x86_64-unknown-linux-gnu': 'space-lens.linux-x64-gnu.node',
    'x86_64-unknown-linux-musl': 'space-lens.linux-x64-musl.node',
    'aarch64-unknown-linux-gnu': 'space-lens.linux-arm64-gnu.node',
    'aarch64-unknown-linux-musl': 'space-lens.linux-arm64-musl.node',
  }
  return targetToArtifact[target] ?? null
}

function currentPlatformArtifact() {
  const arch = process.arch === 'x64' ? 'x64' : process.arch
  if (process.platform === 'win32') {
    return `space-lens.win32-${arch}-msvc.node`
  }
  if (process.platform === 'darwin') {
    return `space-lens.darwin-${arch}.node`
  }
  if (process.platform === 'linux') {
    return `space-lens.linux-${arch}-${isMuslRuntime() ? 'musl' : 'gnu'}.node`
  }
  return `space-lens.${process.platform}-${arch}.node`
}

function isMuslRuntime() {
  if (process.platform !== 'linux') return false
  return !process.report?.getReport?.().header?.glibcVersionRuntime
}
