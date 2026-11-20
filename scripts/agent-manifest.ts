#!/usr/bin/env node
// Pins the Rust CLI release artifacts the desktop is allowed to install on a
// remote host. The manifest is compiled into the desktop binary, so a release
// must be verified against the hashes a shipped build carries — never against
// a digest the download server reports.
//
//   node scripts/agent-manifest.ts --dist dist --version 0.4.0
//   node scripts/agent-manifest.ts --dist dist --check
//
// `dist/` is the release's asset directory (the `spacelens-<target>-v<version>`
// archives). Only archives present in `dist/` are pinned; missing targets are
// reported and keep the manifest usable for the platforms it does carry.
import { createHash } from 'node:crypto'
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const TARGETS = [
  'x86_64-unknown-linux-musl',
  'aarch64-unknown-linux-musl',
  'x86_64-apple-darwin',
  'aarch64-apple-darwin',
  // The release workflow ships the Windows agent as a zip (cargo-binstall's
  // documented override for msvc targets); the manifest accepts both forms.
  'x86_64-pc-windows-msvc',
]

/** The archive extension one release target ships with. */
function archiveName(target: string, version: string): string {
  const extension = target.endsWith('-msvc') && target.startsWith('x86_64-pc-windows') ? 'zip' : 'tar.gz'
  return `spacelens-${target}-v${version}.${extension}`
}

const repoRoot = fileURLToPath(new URL('..', import.meta.url))
const DEFAULT_MANIFEST = join(repoRoot, 'crates/remote/agents/manifest.json')

const args = new Map()
const flags = new Set()
const argv = process.argv.slice(2)
for (let index = 0; index < argv.length; index += 1) {
  const argument = argv[index]
  if (!argument.startsWith('--')) {
    args.set('dist', argument)
    continue
  }
  const [key, inline] = argument.slice(2).split('=', 2)
  if (inline !== undefined) {
    args.set(key, inline)
    continue
  }
  const next = argv[index + 1]
  if (next !== undefined && !next.startsWith('--')) {
    args.set(key, next)
    index += 1
  } else {
    flags.add(key)
  }
}

const dist = resolve(args.get('dist') ?? 'dist')
const manifestPath = resolve(args.get('manifest') ?? DEFAULT_MANIFEST)
const check = flags.has('check')

if (!existsSync(dist)) {
  console.error(`agent-manifest: ${dist} does not exist`)
  process.exit(1)
}

const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'))
const version = args.get('version') ?? manifest.agent.version
if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(`agent-manifest: ${version} is not a semantic version`)
  process.exit(1)
}

const available = new Set(readdirSync(dist))
const targets = {}
const missing = []
for (const target of TARGETS) {
  const archive = archiveName(target, version)
  if (!available.has(archive)) {
    missing.push(target)
    continue
  }
  const path = join(dist, archive)
  const bytes = readFileSync(path)
  targets[target] = {
    archive,
    sha256: createHash('sha256').update(bytes).digest('hex'),
    size: statSync(path).size,
  }
}

if (Object.keys(targets).length === 0) {
  console.error(`agent-manifest: no spacelens-*-v${version} archives in ${dist}`)
  process.exit(1)
}

const next = {
  ...manifest,
  host: { ...manifest.host, version },
  agent: { ...manifest.agent, version, targets },
}

if (check) {
  const current = JSON.stringify(manifest.agent.targets, null, 2)
  const expected = JSON.stringify(targets, null, 2)
  if (manifest.agent.version !== version || manifest.host.version !== version || current !== expected) {
    console.error(`agent-manifest: ${manifestPath} is stale for ${version}`)
    console.error(`  expected:\n${expected}`)
    console.error(`  found:\n${current}`)
    process.exit(1)
  }
  console.log(`agent-manifest: ${manifestPath} matches ${version}`)
} else {
  writeFileSync(manifestPath, `${JSON.stringify(next, null, 2)}\n`)
  console.log(`agent-manifest: pinned ${Object.keys(targets).length} target(s) for ${version}`)
}

if (missing.length > 0) {
  console.log(`agent-manifest: not in ${dist}: ${missing.join(', ')}`)
}
