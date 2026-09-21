#!/usr/bin/env node
// Build order orchestrator: the Tauri shell embeds apps/web/build-desktop at
// compile time, so the web flavor must exist before cargo compiles.
import { execSync } from 'node:child_process'
import { existsSync, rmSync } from 'node:fs'

const webDir = new URL('../apps/web', import.meta.url).pathname
const outDir = `${webDir}/build-desktop`

rmSync(outDir, { recursive: true, force: true })
execSync('yarn build:desktop', { cwd: webDir, stdio: 'inherit' })
if (!existsSync(`${outDir}/index.html`)) {
  console.error('desktop flavor did not produce index.html; refusing to compile the shell')
  process.exit(1)
}
execSync('cargo build --release', { cwd: `${webDir}/../desktop/src-tauri`, stdio: 'inherit' })
console.log('desktop binary compiled; bundle with `npx -y @tauri-apps/cli@2 build` when packaging')
