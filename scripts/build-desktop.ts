#!/usr/bin/env node
// Build order orchestrator: the Tauri shell embeds apps/web/build-desktop at
// compile time, so the web flavor must exist before the shell compiles.
// Mirrors refyard's scripts/build-desktop.ts: frontend first, then the full
// `tauri build` bundle (not just `cargo build`), with bundle verification.
//
//   node scripts/build-desktop.ts            # frontend + app bundle
//   node scripts/build-desktop.ts --web-only # frontend only (frontend-only changes)
import { execSync } from 'node:child_process'
import { existsSync, rmSync, statSync } from 'node:fs'
import { readdirSync } from 'node:fs'
import { join } from 'node:path'

const repoRoot = new URL('..', import.meta.url).pathname
const webDir = join(repoRoot, 'apps/web')
const desktopDir = join(repoRoot, 'apps/desktop')
const outDir = join(webDir, 'build-desktop')
const bundleRoot = join(desktopDir, 'src-tauri/target/release/bundle/macos')
const appBundle = join(bundleRoot, 'Space Lens.app')

const webOnly = process.argv.includes('--web-only')

function run(command, cwd) {
  execSync(command, { cwd, stdio: 'inherit' })
}

function bytesUnder(path) {
  let total = 0
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    const child = join(path, entry.name)
    if (entry.isDirectory()) total += bytesUnder(child)
    else if (entry.isFile()) total += statSync(child).size
  }
  return total
}

// A removed directory rather than an overwritten one: a stale asset from an
// earlier build would be embedded into the app and served as if it were current.
rmSync(outDir, { recursive: true, force: true })
run('yarn build:desktop', webDir)
if (!existsSync(join(outDir, 'index.html'))) {
  console.error('desktop flavor did not produce index.html; refusing to compile the shell')
  process.exit(1)
}

if (!webOnly) {
  run('yarn workspace @space-lens/desktop tauri build', repoRoot)
  if (process.platform === 'darwin') {
    if (!existsSync(appBundle)) {
      throw new Error(`the bundle was not produced at ${appBundle}`)
    }
    console.log(
      `desktop bundle: ${appBundle} (${(bytesUnder(appBundle) / (1024 * 1024)).toFixed(1)} MiB installed)`,
    )
  } else {
    console.log('desktop bundle built (non-macOS runner: skipping .app size report)')
  }
}
