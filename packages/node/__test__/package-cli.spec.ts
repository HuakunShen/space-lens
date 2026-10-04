import test from 'ava'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const packageJson = JSON.parse(readFileSync(resolve(import.meta.dirname, '../package.json'), 'utf8')) as {
  bin?: string | Record<string, string>
  files?: string[]
}

test('space-lens package exposes the TUI as the space-lens executable', (t) => {
  // yarn normalizes a single-entry bin object to the plain-string form on
  // every install; both name the command after the package ("space-lens").
  const bin = packageJson.bin
  const entry = typeof bin === 'string' ? bin : bin?.['space-lens']
  t.is(entry, 'bin/cli.mjs')
  t.true(packageJson.files?.includes('bin'))
})
