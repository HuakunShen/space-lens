import { copyFileSync, existsSync, readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'

const packagePath = resolve('packages/node/package.json')
const backupPath = resolve(process.env.RUNNER_TEMP ?? '/tmp', 'space-lens-node-package.json')
const command = process.argv[2]

if (command === 'strip') {
  copyFileSync(packagePath, backupPath)
  const packageJson = JSON.parse(readFileSync(packagePath, 'utf8'))
  delete packageJson.optionalDependencies
  writeFileSync(packagePath, `${JSON.stringify(packageJson, null, 2)}\n`)
  process.exit(0)
}

if (command === 'restore') {
  if (existsSync(backupPath)) {
    copyFileSync(backupPath, packagePath)
  }
  process.exit(0)
}

console.error('Usage: node scripts/prepare-ci-install.mjs <strip|restore>')
process.exit(1)
