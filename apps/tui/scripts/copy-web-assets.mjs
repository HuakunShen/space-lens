import { cpSync, existsSync, rmSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repoRoot = resolve(packageRoot, '../..')
const source = resolve(repoRoot, 'apps/web/build')
const target = resolve(packageRoot, 'dist/web')

if (!existsSync(resolve(source, 'index.html'))) {
  console.warn(`Skipping web asset copy: ${source} does not contain index.html`)
  process.exit(0)
}

rmSync(target, { recursive: true, force: true })
cpSync(source, target, { recursive: true })
console.log(`Copied web assets to ${target}`)
