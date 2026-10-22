import { test } from 'node:test'
import assert from 'node:assert/strict'
import { fileManagerLabel } from '../src/lib/file-manager.ts'

test('the reveal label names the host platform file manager', () => {
  assert.equal(fileManagerLabel('macos', 'reveal'), 'Reveal in Finder')
  assert.equal(fileManagerLabel('windows', 'reveal'), 'Reveal in Explorer')
  assert.equal(fileManagerLabel('linux', 'reveal'), 'Reveal in File Manager')
})

test('the open label names the host platform file manager', () => {
  assert.equal(fileManagerLabel('macos', 'open'), 'Open in Finder')
  assert.equal(fileManagerLabel('windows', 'open'), 'Open in Explorer')
  assert.equal(fileManagerLabel('linux', 'open'), 'Open in File Manager')
})

test('reveal is the default action', () => {
  assert.equal(fileManagerLabel('macos'), 'Reveal in Finder')
})

test('an unknown or missing platform falls back to a generic label', () => {
  assert.equal(fileManagerLabel('unknown', 'reveal'), 'Reveal in file manager')
  assert.equal(fileManagerLabel(undefined), 'Reveal in file manager')
  assert.equal(fileManagerLabel(null, 'open'), 'Open in file manager')
})
