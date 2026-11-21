import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'

import { freedesktopTrash, trashInfo } from '../src/trash.ts'

let roots: string[] = []

function makeRoot(): string {
  const root = mkdtempSync(join(tmpdir(), 'space-lens-trash-'))
  roots.push(root)
  return root
}

afterEach(() => {
  for (const root of roots) rmSync(root, { recursive: true, force: true })
  roots = []
})

describe('freedesktop trash fallback', () => {
  it('moves a file and records its original location', async () => {
    const root = makeRoot()
    const source = join(root, 'project')
    mkdirSync(source)
    const file = join(source, 'index.js')
    writeFileSync(file, 'export {}\n')

    const results = await freedesktopTrash([file], join(root, 'Trash'))

    expect(results.get(file)).toBeNull()
    expect(readFileSync(join(root, 'Trash/files/index.js'), 'utf8')).toBe('export {}\n')
    const info = readFileSync(join(root, 'Trash/info/index.js.trashinfo'), 'utf8')
    expect(info).toContain(`Path=${file}`)
    expect(info).toContain('DeletionDate=')
  })

  it('moves a directory and keeps its contents', async () => {
    const root = makeRoot()
    const source = join(root, 'node_modules')
    mkdirSync(join(source, 'express'), { recursive: true })
    writeFileSync(join(source, 'express/index.js'), 'x')

    const results = await freedesktopTrash([source], join(root, 'Trash'))

    expect(results.get(source)).toBeNull()
    expect(readFileSync(join(root, 'Trash/files/node_modules/express/index.js'), 'utf8')).toBe('x')
  })

  it('suffixed names never overwrite an earlier trashed entry', async () => {
    const root = makeRoot()
    const firstParent = join(root, 'one')
    const secondParent = join(root, 'two')
    mkdirSync(firstParent, { recursive: true })
    mkdirSync(secondParent, { recursive: true })
    const first = join(firstParent, 'a')
    const second = join(secondParent, 'a')
    writeFileSync(first, 'one')
    writeFileSync(second, 'two')

    const trash = join(root, 'Trash')
    await freedesktopTrash([first], trash)
    const results = await freedesktopTrash([second], trash)

    expect(results.get(second)).toBeNull()
    expect(readFileSync(join(trash, 'files/a'), 'utf8')).toBe('one')
    expect(readFileSync(join(trash, 'files/a 2'), 'utf8')).toBe('two')
    expect(readFileSync(join(trash, 'info/a 2.trashinfo'), 'utf8')).toContain(`Path=${second}`)
  })

  it('reports a vanished path instead of throwing', async () => {
    const root = makeRoot()
    const missing = join(root, 'gone')

    const results = await freedesktopTrash([missing], join(root, 'Trash'))

    expect(results.get(missing)).toContain('vanished')
  })

  it('percent-encodes the original path except separators', () => {
    expect(trashInfo('/home/al ice/it\'s', new Date('2026-11-20T10:20:30Z'))).toBe(
      "[Trash Info]\nPath=/home/al%20ice/it's\nDeletionDate=2026-11-20T10:20:30\n",
    )
  })
})

describe('windows recycle bin verification', () => {
  it('matches the plain name and the duplicate rename, and escapes brackets', async () => {
    const { recycleBinNamePattern } = await import('../src/trash.ts')
    // Windows appends " (2)" for a second item with the same display name.
    expect(recycleBinNamePattern('node_modules')).toBe('node_modules (*')
    // `-like` would read [a] as a character class and count other entries.
    const escaped = recycleBinNamePattern('cache[old]')
    expect(escaped).toContain('`[')
    expect(escaped).toContain('`]')
  })
})
