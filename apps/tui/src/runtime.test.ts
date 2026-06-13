import assert from 'node:assert/strict'
import test from 'node:test'

import { assertSupportedRuntime, isBunRuntime } from './runtime.js'

test('isBunRuntime detects whether Bun is available on the runtime global', () => {
  assert.equal(isBunRuntime({ Bun: {} }), true)
  assert.equal(isBunRuntime({}), false)
})

test('assertSupportedRuntime explains the OpenTUI Bun requirement', () => {
  assert.doesNotThrow(() => assertSupportedRuntime({ Bun: {} }))
  assert.throws(
    () => assertSupportedRuntime({}),
    /OpenTUI currently requires Bun. Run this CLI with `bun` or `yarn tui`./,
  )
})
