// Unit tests for the memory view's reason and pressure wording (T17), including the
// banned-words guard from §7 of the memory-cleaner plan.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import type { CandidateReason } from '@space-lens/contract'
import { BANNED_WORDS, pressureHeadline, reasonText } from '../src/lib/memory/reasons.ts'

const MB = 1024 * 1024
const GB = 1024 * MB

function reason(overrides: Partial<CandidateReason> & { code: CandidateReason['code'] }): CandidateReason {
  return { bytes: null, minutes: null, cpuPercent: null, count: null, fromBytes: null, ...overrides }
}

test('reason text matches the wording in §7', () => {
  assert.equal(reasonText(reason({ code: 'large', bytes: 757 * MB })), 'Using 757 MB')
  assert.equal(
    reasonText(reason({ code: 'idle', minutes: 6, cpuPercent: 0.4 })),
    'No activity for 6 min while you watched (0.4% CPU)',
  )
  assert.equal(reasonText(reason({ code: 'background' })), 'Runs in the background without a Dock icon')
  assert.equal(
    reasonText(reason({ code: 'growing', fromBytes: 512 * MB, bytes: 900 * MB, minutes: 10 })),
    'Grew from 512 MB to 900 MB in 10 min',
  )
  assert.equal(reasonText(reason({ code: 'manyProcesses', count: 12 })), '12 processes')
})

test('reason text is defensive about missing numbers', () => {
  assert.equal(reasonText(reason({ code: 'large' })), 'Using 0 B')
  assert.equal(reasonText(reason({ code: 'idle' })), 'No activity for 0 min while you watched (0% CPU)')
  assert.equal(reasonText(reason({ code: 'growing' })), 'Grew from 0 B to 0 B in 0 min')
  assert.equal(reasonText(reason({ code: 'manyProcesses' })), '0 processes')
})

test('the pressure headline matches the wording in §7', () => {
  assert.equal(
    pressureHeadline('normal', 0),
    'Memory pressure is normal. macOS keeps spare memory busy as cache, which makes your Mac faster — there is nothing that needs freeing. If something feels slow, quitting an idle app below is the only change that really helps.',
  )
  assert.equal(
    pressureHeadline('warning', 2 * GB),
    'Memory is tight: macOS is compressing memory and may use swap. Quitting the apps below would give back about 2.0 GB.',
  )
  assert.equal(
    pressureHeadline('critical', 2 * GB),
    'Memory is very tight and apps may slow down or be paused. Quitting the apps below would give back about 2.0 GB.',
  )
  assert.ok(pressureHeadline('unknown', 0).length > 0)
})

test('reasons.ts never uses a banned word outside the BANNED_WORDS list (§7)', () => {
  const source = readFileSync(new URL('../src/lib/memory/reasons.ts', import.meta.url), 'utf8')
  const withoutList = source.replace(/export const BANNED_WORDS[\s\S]*?as const/, '')
  for (const word of BANNED_WORDS) {
    assert.equal(
      withoutList.toLowerCase().includes(word),
      false,
      `reasons.ts uses the banned word "${word}" outside BANNED_WORDS`,
    )
  }
  assert.deepEqual([...BANNED_WORDS], ['clean', 'boost', 'optimiz', 'purge', 'speed up'])
})
