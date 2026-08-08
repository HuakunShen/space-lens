import test from 'ava'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const packageJson = JSON.parse(readFileSync(resolve(import.meta.dirname, '../package.json'), 'utf8')) as {
  bin?: Record<string, string>
  files?: string[]
}

test('space-lens package exposes the TUI as the space-lens executable', (t) => {
  t.is(packageJson.bin?.['space-lens'], 'bin/cli.mjs')
  t.true(packageJson.files?.includes('bin'))
})
