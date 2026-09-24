/**
 * Build the DeepSeek Harness plugin bundle for Space Lens.
 *
 * Three artifacts, each for the reason the others do not cover (the Refyard
 * plugin's build established this shape):
 *
 * 1. **The embedded SPA** (`apps/web/build-embed`) — the same SvelteKit app built
 *    for a mount prefix (`SPACLENS_EMBED_BASE`) and without a service worker.
 *    Both differences are baked into the document the browser receives, so they
 *    must be a build, not a patch of served bytes.
 * 2. **The host half** (`dist/host.js`) — one ESM file the Harness host process
 *    loads once and caches by URL. Workspace packages are bundled in, because
 *    the installed plugin must not depend on `workspace:*` specifiers resolving
 *    inside the Harness profile. The one thing that must *not* be bundled is the
 *    napi engine: it loads a platform `.node` binary by path. Instead the
 *    resolved `space-lens` package is copied to `dist/node_modules/space-lens`,
 *    where Node's own resolution — walking up from `dist/host.js` — finds it.
 * 3. **The client half** (`dist/client.js`) — a classic script registering itself
 *    with the Web shell's module table. Not ESM: the shell loads bundle scripts,
 *    and a top-level `import` would never run.
 *
 * Bundles are written only when their bytes changed: the Harness re-reads a
 * bundle on mtime and remembers a bad read as failed for the life of the
 * process, so a rebuild that rewrites identical bytes can poison the panel.
 */
import { spawnSync } from 'node:child_process'
import { createRequire } from 'node:module'
import { cp, mkdir, readFile, rm, stat, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { build } from 'esbuild'

const repositoryRoot = join(dirname(fileURLToPath(import.meta.url)), '..')
const pluginRoot = join(repositoryRoot, 'apps', 'dsh')
const distRoot = join(pluginRoot, 'dist')
const webApp = join(repositoryRoot, 'apps', 'web')
const webSource = join(webApp, 'build-embed')

/** The mount prefix the SPA is built for. The host half mounts the workbench here. */
const EMBED_BASE = '/space-lens'

function run(argv, cwd) {
  const [command, ...args] = argv
  const result = spawnSync(command, args, { cwd, stdio: 'inherit', env: process.env })
  if (result.error !== undefined) throw result.error
  if (result.status !== 0) throw new Error(`${argv.join(' ')} exited with ${String(result.status)}`)
}

/** esbuild transpiles without checking, so the plugin's own types are verified here. */
function checkTypes() {
  const tsc = join(repositoryRoot, 'node_modules', '.bin', 'tsc')
  for (const project of ['tsconfig.json', 'tsconfig.client.json']) {
    run([tsc, '--noEmit', '-p', join(pluginRoot, project)], repositoryRoot)
  }
}

async function buildSpa() {
  process.env.SPACLENS_BUILD_TARGET = 'embed'
  process.env.SPACLENS_EMBED_BASE = EMBED_BASE
  const vite = join(repositoryRoot, 'node_modules', '.bin', 'vite')
  run([vite, 'build'], webApp)
}

/** Write only when bytes changed — see the module comment. Returns whether the file moved. */
async function writeIfChanged(path, contents) {
  const previous = await readFile(path, 'utf8').catch(() => null)
  if (previous === contents) return false
  await writeFile(path, contents, 'utf8')
  return true
}

async function emit(options, outfile) {
  const result = await build({ ...options, outfile, write: false })
  if (result.errors.length > 0) throw new Error(`${outfile}: ${result.errors.length} error(s)`)
  const written = result.outputFiles?.[0]
  if (written === undefined) throw new Error(`${outfile}: the bundler produced no output`)
  const changed = await writeIfChanged(outfile, written.text)
  console.log(`build-dsh-plugin: ${outfile} ${changed ? 'written' : 'unchanged'}`)
}

/**
 * Stage the napi engine beside the host bundle. `ScanManager` resolves
 * `space-lens` with `createRequire(import.meta.url)` from inside the bundle, so
 * Node walks up from `dist/` and finds it in the plugin root's `node_modules`.
 *
 * The plugin root (not `dist/node_modules`) is deliberate: npm excludes a
 * package's own root `node_modules` from tarballs unconditionally, while a
 * nested `dist/node_modules` would slip past both `files` and `.npmignore`.
 * The published package instead declares `space-lens` as a dependency, so npm
 * installs resolve it from the profile's own tree — every platform gets its
 * own binary, never this build machine's.
 */
async function stageEngine() {
  const require = createRequire(join(repositoryRoot, 'package.json'))
  const engineRoot = dirname(require.resolve('space-lens/package.json'))
  const target = join(pluginRoot, 'node_modules', 'space-lens')
  await rm(target, { recursive: true, force: true })
  await mkdir(target, { recursive: true })
  const wanted = (await stat(join(engineRoot, 'index.js'))).isFile()
  if (!wanted) throw new Error('the space-lens package has no index.js')
  for (const name of ['package.json', 'index.js', 'index.d.ts']) {
    await cp(join(engineRoot, name), join(target, name))
  }
  // The platform binary: whatever `.node` files the local build produced.
  const { readdir } = await import('node:fs/promises')
  for (const entry of await readdir(engineRoot)) {
    if (entry.endsWith('.node')) await cp(join(engineRoot, entry), join(target, entry))
  }
}

async function buildHost() {
  await emit(
    {
      entryPoints: [join(pluginRoot, 'src', 'host.ts')],
      bundle: true,
      platform: 'node',
      format: 'esm',
      target: 'node22',
      // Workspace packages are bundled; the napi engine is never imported at
      // runtime from here (type-only), so nothing needs to stay external.
      external: [],
      banner: { js: '// Generated by `scripts/build-dsh-plugin.mjs`; not source. Rebuild rather than edit.' },
      logLevel: 'info',
    },
    join(distRoot, 'host.js'),
  )
}

async function buildClient() {
  await emit(
    {
      entryPoints: [join(pluginRoot, 'src', 'client.ts')],
      bundle: true,
      // The browser module table supplies React; bundling a second copy would give
      // the panel a different React instance than the shell rendering it.
      external: ['react', 'react/jsx-runtime'],
      platform: 'browser',
      format: 'iife',
      target: 'es2022',
      banner: { js: '// Generated by `scripts/build-dsh-plugin.mjs`; not source. Rebuild rather than edit.' },
      logLevel: 'info',
    },
    join(distRoot, 'client.js'),
  )
}

checkTypes()
await stageEngine()
await buildSpa()

const spaInfo = await stat(join(webSource, '200.html')).catch(() => null)
if (spaInfo?.isFile() !== true) {
  throw new Error('apps/web/build-embed/200.html is missing; the embedded SPA build produced nothing')
}

// Only the staged SPA is replaced wholesale; the two module files are overwritten
// in place and never removed — a running Harness serves `client.js` from here,
// and a rebuild that deletes it takes the panel's registrations with it, silently.
await rm(join(distRoot, 'web'), { recursive: true, force: true })
await mkdir(distRoot, { recursive: true })
await cp(webSource, join(distRoot, 'web'), { recursive: true })
await buildHost()
await buildClient()

await writeFile(
  join(distRoot, 'BUILD.txt'),
  [`Generated by \`scripts/build-dsh-plugin.mjs\`.`, `mount prefix: ${EMBED_BASE}`, ''].join('\n'),
  'utf8',
)

console.log(`build-dsh-plugin: staged ${distRoot} and ${join(distRoot, 'web')}`)
