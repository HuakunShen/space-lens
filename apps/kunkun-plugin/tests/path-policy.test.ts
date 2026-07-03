/**
 * Tests for the Space Lens Kunkun backend's local filesystem path policy.
 * These checks complement Kunkun host permissions by proving the backend
 * rejects traversal, sibling-prefix, malformed, and symlink escape paths.
 */
import { afterEach, describe, expect, test } from 'bun:test'
import { mkdtempSync, mkdirSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'

import {
  BACKEND_FS_READ_ROOTS_ENV,
  assertPathUnderAllowedRoots,
  normalizeAllowedRoots,
  readAllowedRootsFromEnv,
} from '../src/path-policy'

const tempDirs: string[] = []

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true })
  }
})

describe('path policy', () => {
  test('normalizes duplicate recursive roots and accepts descendants', () => {
    const root = tempDir('space-lens-policy-root-')
    const child = path.join(root, 'child')
    mkdirSync(child)

    const roots = normalizeAllowedRoots([`${root}/**`, root])

    expect(roots).toEqual([realpathSync.native(root)])
    expect(() => assertPathUnderAllowedRoots(child, roots)).not.toThrow()
  })

  test('rejects missing roots, relative paths, traversal, and sibling-prefix escapes', () => {
    const root = tempDir('space-lens-policy-allowed-')
    const sibling = tempDir('space-lens-policy-allowed-sibling-')
    const roots = normalizeAllowedRoots([root])

    expect(() => assertPathUnderAllowedRoots(root, [])).toThrow(/No approved filesystem roots/)
    expect(() => normalizeAllowedRoots(['relative/root'])).toThrow(/must be absolute/)
    expect(() => assertPathUnderAllowedRoots('relative/file', roots)).toThrow(/must be absolute/)
    expect(() => assertPathUnderAllowedRoots(path.join(root, '..', path.basename(sibling)), roots)).toThrow(
      /outside the approved/,
    )
    expect(() => assertPathUnderAllowedRoots(`${root}-not-really-inside`, roots)).toThrow(/outside the approved/)
  })

  test('rejects symlink escapes outside the approved root', () => {
    const root = tempDir('space-lens-policy-root-')
    const outside = tempDir('space-lens-policy-outside-')
    const outsideFile = path.join(outside, 'secret.txt')
    writeFileSync(outsideFile, 'secret')

    const link = path.join(root, 'linked-secret.txt')
    symlinkSync(outsideFile, link)
    const roots = normalizeAllowedRoots([root])

    expect(() => assertPathUnderAllowedRoots(link, roots)).toThrow(/outside the approved/)
  })

  test('parses allowed roots from env and fails closed on malformed values', () => {
    const root = tempDir('space-lens-policy-env-')

    expect(readAllowedRootsFromEnv({ [BACKEND_FS_READ_ROOTS_ENV]: JSON.stringify([root]) })).toEqual([root])
    expect(() => readAllowedRootsFromEnv({ [BACKEND_FS_READ_ROOTS_ENV]: '"not-array"' })).toThrow(
      /JSON string array/,
    )
    expect(() => readAllowedRootsFromEnv({ [BACKEND_FS_READ_ROOTS_ENV]: 'not-json' })).toThrow(/not valid JSON/)
  })
})

function tempDir(prefix: string): string {
  const dir = mkdtempSync(path.join(tmpdir(), prefix))
  tempDirs.push(dir)
  return dir
}
