#!/usr/bin/env node
// Stands in for the protected `spacelens scan --stream-nodes` CLI: streams
// SYNTH_NODES deterministic preorder nodes under the requested root, honouring
// pipe backpressure. SYNTH_EXACT=1 adds cache and virtualenv names.
const path = require('node:path')
const args = process.argv.slice(2)
const root = args[1]
const budget = Number(process.env.SYNTH_NODES || 1000000)
let seed = 12345
const rand = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff
const words = [
  'index',
  'package',
  'node_modules',
  'src',
  'lib',
  'Contents',
  'Resources',
  'README',
  'component',
  'utils',
  'config',
  'build',
  'cache',
  'Library',
  'Application Support',
  'com.apple.metadata',
  'Frameworks',
  'test',
  '__pycache__',
  'target',
]
const exact = !!process.env.SYNTH_EXACT
if (exact) words.push('.venv', '.next', 'venv', '.svelte-kit', '.pytest_cache')
const exts = ['js', 'ts', 'json', 'png', 'dylib', 'plist', 'md', 'py', 'pyc', 'rs', 'strings', 'nib']
let emitted = 0,
  files = 0,
  dirs = 0
const out = []
const line = (p, depth, isDir) => {
  const size = isDir ? 0 : Math.floor(rand() * 200000) * 512
  emitted++
  if (isDir) dirs++
  else files++
  out.push(
    JSON.stringify({
      type: 'node',
      node: {
        name: path.basename(p),
        path: p,
        size,
        children: [],
        depth,
        ignored: rand() < 0.03,
        collapsed: false,
        logicalSize: size,
        isDirectory: isDir,
        scanState: 'complete',
        skipReason: null,
      },
    }) + '\n',
  )
}
function* visit(p, depth, isDir) {
  line(p, depth, isDir)
  if (out.length >= 2000) yield out.splice(0).join('')
  if (!isDir) return
  if (rand() < 0.2 && emitted < budget) yield* visit(path.join(p, 'package.json'), depth + 1, false)
  if (exact && rand() < 0.1 && emitted < budget) yield* visit(path.join(p, 'Cargo.toml'), depth + 1, false)
  if (exact && rand() < 0.03 && emitted < budget) yield* visit(path.join(p, 'pyvenv.cfg'), depth + 1, false)
  const nd = depth >= 9 ? 0 : Math.floor(rand() * 5)
  const nf = Math.floor(rand() * 20)
  for (let i = 0; i < nf && emitted < budget; i++)
    yield* visit(
      path.join(p, `${words[Math.floor(rand() * words.length)]}-${i}.${exts[Math.floor(rand() * exts.length)]}`),
      depth + 1,
      false,
    )
  for (let i = 0; i < nd && emitted < budget; i++)
    yield* visit(
      path.join(p, `${words[Math.floor(rand() * words.length)]}${exact && i === 0 ? '' : i}`),
      depth + 1,
      true,
    )
}
function* all() {
  line(root, 0, true)
  for (let i = 0; emitted < budget; i++) yield* visit(path.join(root, `top${i}`), 1, true)
  yield out.splice(0).join('')
}
const done = () => {
  const coverage = {
    mode: 'local-only',
    protection: process.platform === 'darwin' ? 'macos-no-materialization' : 'metadata-only',
    sizeMetric: 'allocated',
    logicalBytes: 0,
    files,
    directories: dirs,
    skippedCount: 0,
    deniedCount: 0,
    issueCount: 0,
    issuesTruncated: false,
    issues: [],
    elapsedMs: 1,
  }
  process.stdout.write(JSON.stringify({ type: 'done', report: { nodes: [], coverage, volumes: [] } }) + '\n')
}
const it = all()
const pump = () => {
  for (;;) {
    const next = it.next()
    if (next.done) return done()
    if (!process.stdout.write(next.value)) return void process.stdout.once('drain', pump)
  }
}
pump()
