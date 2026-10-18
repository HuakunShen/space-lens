// Unit tests for the client-side Quit list reducer (T20, §6.2 item 6): dedupe
// by key, newest wins, and the ≤ 20 cap.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { addToQuitList, QUIT_LIST_CAP, type QuitListEntry } from '../src/lib/memory/quit-list.ts'

function entry(key: string, overrides: Partial<QuitListEntry> = {}): QuitListEntry {
  return {
    key,
    groupId: 'grp_0123456789abcdef',
    name: key,
    origin: 'manual',
    reason: null,
    stagedAt: '2026-10-18T00:00:00.000Z',
    ...overrides,
  }
}

test('de-duplicates by key and keeps the newest entry', () => {
  const first = entry('group:a', { reason: 'first' })
  const second = entry('group:a', { reason: 'second', origin: 'advisor' })
  const list = addToQuitList(addToQuitList([], first), second)
  assert.equal(list.length, 1)
  assert.equal(list[0].reason, 'second')
  assert.equal(list[0].origin, 'advisor')
})

test('re-staging an existing key moves it to the end', () => {
  const a = entry('group:a')
  const b = entry('group:b')
  const restaged = entry('group:a', { reason: 'again' })
  const list = addToQuitList(addToQuitList(addToQuitList([], a), b), restaged)
  assert.deepEqual(
    list.map((e) => e.key),
    ['group:b', 'group:a'],
  )
})

test('caps the list at 20, dropping the oldest entries', () => {
  let list: QuitListEntry[] = []
  for (let index = 0; index < 25; index += 1) list = addToQuitList(list, entry(`group:${index}`))
  assert.equal(list.length, QUIT_LIST_CAP)
  assert.equal(list[0].key, 'group:5')
  assert.equal(list.at(-1)?.key, 'group:24')
})

test('never mutates the input list', () => {
  const input: readonly QuitListEntry[] = [entry('group:a')]
  const next = addToQuitList(input, entry('group:b'))
  assert.equal(input.length, 1)
  assert.equal(next.length, 2)
})
