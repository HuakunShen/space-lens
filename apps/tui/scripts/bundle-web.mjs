#!/usr/bin/env node
// Bundles the web workbench (apps/web's browser build) beside the CLI entry,
// where `spacelens serve` looks for it: `<dist>/web/200.html`. A host package
// without the UI is still a working API, but `spacelens remote open` and the
// pairing URL exist precisely so a browser can drive it, so the published
// package carries the workbench.
import { cpSync, existsSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const packageRoot = fileURLToPath(new URL('..', import.meta.url))
const webBuild = join(packageRoot, '../web/build')
const target = join(packageRoot, 'dist/web')

for (const required of ['200.html', 'index.html']) {
  if (!existsSync(join(webBuild, required))) {
    console.error(
      `bundle-web: apps/web/build is missing ${required} — run \`yarn workspace @space-lens/web build\` first`,
    )
    process.exit(1)
  }
}

rmSync(target, { recursive: true, force: true })
cpSync(webBuild, target, { recursive: true })
console.log(`bundle-web: dist/web now serves the workbench (${target})`)
