import { chmodSync, cpSync, existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'

const packageRoot = resolve(import.meta.dirname, '..')
const sourceRoot = resolve(packageRoot, '../../apps/tui/dist')
const targetRoot = resolve(packageRoot, 'bin')
const sourceCli = resolve(sourceRoot, 'cli.mjs')

if (!existsSync(sourceCli)) {
  throw new Error(`TUI CLI bundle is missing: ${sourceCli}. Build @space-lens/cli first.`)
}

rmSync(targetRoot, { recursive: true, force: true })
mkdirSync(targetRoot, { recursive: true })
cpSync(sourceCli, resolve(targetRoot, 'cli.mjs'))
cpSync(resolve(sourceRoot, 'assets'), resolve(targetRoot, 'assets'), { recursive: true })
writeFileSync(resolve(targetRoot, 'package.json'), '{\n  "type": "module"\n}\n')
chmodSync(resolve(targetRoot, 'cli.mjs'), 0o755)
