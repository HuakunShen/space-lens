// Unit tests for the memory view's reason and pressure wording (T17) and for the
// banned-words guard from §7 of the memory-cleaner plan, extended by T19 to cover
// every `components/memory/*.svelte` action component.
import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { test } from 'node:test'
import type { CandidateReason } from '@space-lens/contract'
import { BANNED_WORDS, pressureHeadline, reasonText } from '../src/lib/memory/reasons.ts'

const MB = 1024 * 1024
const GB = 1024 * MB

/** The components/memory directory URL, with a trailing slash so `new URL(name, dir)` resolves. */
const MEMORY_COMPONENTS_DIR = new URL('../src/components/memory/', import.meta.url)

/** The four action components this task ships (T19). */
const ACTION_COMPONENTS = [
  'QuitListPanel.svelte',
  'TerminateDialog.svelte',
  'FreeUpDialog.svelte',
  'AuditLogPanel.svelte',
] as const

/**
 * §7 bans "clean", "boost", "optimize", "purge" (as an action) and "speed up"
 * from the memory UI. The always-on footer is the single sanctioned exception:
 * it names the "memory purge" trick only to say Space Lens does not run it.
 * Removing that phrase keeps the guard strict for every other use.
 */
const SANCTIONED_FOOTER_PHRASE = "It does not run 'memory purge' tricks"

function memoryComponents(): string[] {
  return readdirSync(MEMORY_COMPONENTS_DIR).filter((name) => name.endsWith('.svelte'))
}

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

test('the memory action components ship (§6.2/§6.3)', () => {
  const present = memoryComponents()
  for (const component of ACTION_COMPONENTS) {
    assert.ok(present.includes(component), `components/memory/${component} is missing`)
  }
})

test('no memory component uses a banned word outside the sanctioned footer (§7)', () => {
  const names = memoryComponents()
  assert.ok(names.length > 0)
  for (const name of names) {
    const source = readFileSync(new URL(name, MEMORY_COMPONENTS_DIR), 'utf8')
    const scrubbed = source.split(SANCTIONED_FOOTER_PHRASE).join('')
    for (const word of BANNED_WORDS) {
      assert.equal(
        scrubbed.toLowerCase().includes(word),
        false,
        `components/memory/${name} uses the banned word "${word}"`,
      )
    }
  }
})

/**
 * Sentences §6.3/§7 pins verbatim. The dialog builds them from interpolated
 * values, so the test checks the fixed fragments that must survive unchanged.
 */
const REQUIRED_WORDING: ReadonlyArray<{ file: string; phrases: readonly string[] }> = [
  {
    file: 'TerminateDialog.svelte',
    phrases: [
      'I understand unsaved work in these apps will be lost',
      'Still open — it may be asking you to save changes. Check its window, or force quit.',
      'Released about',
      'Used memory went from',
      'over the next',
    ],
  },
  {
    file: 'FreeUpDialog.svelte',
    phrases: ['No idle large apps found', "It does not run 'memory purge' tricks"],
  },
]

test('the action components carry the required §6.3/§7 wording verbatim', () => {
  for (const { file, phrases } of REQUIRED_WORDING) {
    const source = readFileSync(new URL(file, MEMORY_COMPONENTS_DIR), 'utf8')
    for (const phrase of phrases) {
      assert.ok(source.includes(phrase), `components/memory/${file} is missing "${phrase}"`)
    }
  }
})
