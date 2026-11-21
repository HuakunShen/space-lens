import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'

import { listFolder } from '../src/fs-list.ts'

/**
 * The picker browses one folder at a time inside the roots a host serves. The
 * containment rule is the same one scans apply (decided on the real path), so
 * these tests pin the refusals as much as the listing.
 */
describe('folder listing for the scan picker', () => {
  let base: string
  let root: string
  let outside: string

  beforeAll(() => {
    base = mkdtempSync(join(tmpdir(), 'space-lens-fs-list-'))
    root = join(base, 'root')
    outside = join(base, 'outside')
    mkdirSync(join(root, 'alpha', 'nested'), { recursive: true })
    mkdirSync(join(root, 'beta'), { recursive: true })
    mkdirSync(outside, { recursive: true })
    writeFileSync(join(root, 'notes.txt'), 'four')
    writeFileSync(join(outside, 'secret.txt'), 'nope')
    // A symlink inside the root that points outside it: visible as a row,
    // but entering it must be refused.
    symlinkSync(outside, join(root, 'escape'))
  })

  afterAll(() => rmSync(base, { recursive: true, force: true }))

  it('lists folders first with their own sizes', async () => {
    const page = await listFolder({ path: root }, [root])
    expect(page.path).toBe(resolve(root))
    expect(page.roots).toEqual([resolve(root)])
    expect(page.parent).toBeNull()
    expect(page.truncated).toBe(false)
    expect(page.entries.map((entry) => entry.name)).toEqual(['alpha', 'beta', 'escape', 'notes.txt'])
    expect(page.entries.map((entry) => entry.kind)).toEqual(['dir', 'dir', 'dir', 'file'])
    const notes = page.entries.find((entry) => entry.name === 'notes.txt')
    expect(notes?.size).toBe(4)
    expect(notes?.path).toBe(join(resolve(root), 'notes.txt'))
  })

  it('walks down and reports the parent, but never above a root', async () => {
    const nested = await listFolder({ path: join(root, 'alpha') }, [root])
    expect(nested.path.endsWith('alpha')).toBe(true)
    expect(nested.parent).toBe(resolve(root))
    expect(nested.entries.map((entry) => entry.name)).toEqual(['nested'])

    const atRoot = await listFolder({ path: join(root, 'alpha', 'nested') }, [root])
    expect(atRoot.entries).toEqual([])
  })

  it('refuses a folder outside the roots', async () => {
    await expect(listFolder({ path: outside }, [root])).rejects.toThrow(/outside the locations this host serves/)
  })

  it('refuses a symlink that points outside the roots', async () => {
    await expect(listFolder({ path: join(root, 'escape') }, [root])).rejects.toThrow(
      /outside the locations this host serves/,
    )
  })

  it('reports a missing folder and a file path distinctly', async () => {
    await expect(listFolder({ path: join(root, 'missing') }, [root])).rejects.toThrow(/does not exist/)
    await expect(listFolder({ path: join(root, 'notes.txt') }, [root])).rejects.toThrow(/not a folder/)
  })

  it('caps the page and says it was truncated', async () => {
    const page = await listFolder({ path: root, limit: 2 }, [root])
    expect(page.entries).toHaveLength(2)
    expect(page.truncated).toBe(true)
  })

  it('defaults to the first served root and expands a leading tilde', async () => {
    const page = await listFolder({}, [root])
    expect(page.path).toBe(resolve(root))
    // `~` is the process home, which is not this fixture's root: the listing
    // is refused rather than silently reading some other folder.
    await expect(listFolder({ path: '~' }, [root])).rejects.toThrow(/outside the locations this host serves/)
  })

  it('keeps nested roots separate so the breadcrumb stops at the declared one', async () => {
    const nestedRoot = join(root, 'alpha')
    const page = await listFolder({ path: nestedRoot }, [root, nestedRoot])
    expect(page.parent).toBe(resolve(root))
  })
})
