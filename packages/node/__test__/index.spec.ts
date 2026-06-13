import test from 'ava'
import { existsSync, linkSync, mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { executeCleanup, findCleanupCandidates, planCleanup, scanDirectory } from '../index'
import type { CleanupCandidate, DirectoryNode } from '../index'

function createGitignoreFixture() {
  const root = mkdtempSync(join(tmpdir(), 'space-lens-compact-'))
  mkdirSync(join(root, 'src'))
  mkdirSync(join(root, 'target'))
  mkdirSync(join(root, 'target', 'debug'))
  mkdirSync(join(root, 'node_modules'))
  mkdirSync(join(root, 'node_modules', 'left-pad'))
  writeFileSync(join(root, '.gitignore'), 'target/\nnode_modules/\n')
  writeFileSync(join(root, 'src', 'index.ts'), "console.log('hello')\n")
  writeFileSync(join(root, 'target', 'debug', 'app.bin'), 'x'.repeat(4096))
  writeFileSync(join(root, 'node_modules', 'left-pad', 'index.js'), "module.exports = ''\n")

  return root
}

function childByName(node: DirectoryNode, name: string) {
  return node.children.find((child) => child.name === name)
}

test('scanDirectory summarizes gitignored directories without descendants', (t) => {
  const root = createGitignoreFixture()
  t.teardown(() => rmSync(root, { recursive: true, force: true }))

  const [tree] = scanDirectory({
    directories: [root],
    fullPath: false,
    respectGitignore: true,
    ignoredMode: 'summarize',
  })

  const target = childByName(tree, 'target')
  const nodeModules = childByName(tree, 'node_modules')

  t.truthy(childByName(tree, 'src'))
  t.truthy(target)
  t.truthy(nodeModules)
  t.true(target!.size > 0)
  t.true(nodeModules!.size > 0)
  t.true(target!.ignored)
  t.true(target!.collapsed)
  t.deepEqual(target!.children, [])
  t.true(nodeModules!.ignored)
  t.true(nodeModules!.collapsed)
  t.deepEqual(nodeModules!.children, [])
})

test('scanDirectory excludes gitignored directories when requested', (t) => {
  const root = createGitignoreFixture()
  t.teardown(() => rmSync(root, { recursive: true, force: true }))

  const [tree] = scanDirectory({
    directories: [root],
    fullPath: false,
    respectGitignore: true,
    ignoredMode: 'exclude',
  })

  t.truthy(childByName(tree, 'src'))
  t.falsy(childByName(tree, 'target'))
  t.falsy(childByName(tree, 'node_modules'))
})

test('scanDirectory does not double count hard links', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'space-lens-hardlink-'))
  t.teardown(() => rmSync(root, { recursive: true, force: true }))

  const original = join(root, 'original.bin')
  const duplicate = join(root, 'duplicate.bin')
  writeFileSync(original, 'x'.repeat(4096))

  try {
    linkSync(original, duplicate)
  } catch {
    t.pass()
    return
  }

  const [tree] = scanDirectory({
    directories: [root],
    fullPath: false,
    respectGitignore: false,
  })

  const linkedFiles = tree.children.filter((child) => ['original.bin', 'duplicate.bin'].includes(child.name))

  t.is(linkedFiles.length, 1)
})

test('findCleanupCandidates reports preset matches', (t) => {
  const root = createGitignoreFixture()
  t.teardown(() => rmSync(root, { recursive: true, force: true }))

  const candidates = findCleanupCandidates({
    directories: [root],
    presets: ['node', 'rust', 'gitignored'],
  })

  t.truthy(candidateEndingWith(candidates, 'node_modules'))
  t.truthy(candidateEndingWith(candidates, 'target'))
})

test('planCleanup returns a dry-run removal plan', (t) => {
  const root = createGitignoreFixture()
  t.teardown(() => rmSync(root, { recursive: true, force: true }))

  const plan = planCleanup({
    directories: [root],
    presets: ['node'],
  })

  t.is(plan.entries.length, 1)
  t.true(plan.totalSize > 0)
  t.true(plan.entries[0].path.endsWith('node_modules'))
})

test('executeCleanup removes entries from an explicit plan', (t) => {
  const root = createGitignoreFixture()
  t.teardown(() => rmSync(root, { recursive: true, force: true }))

  const plan = planCleanup({
    directories: [root],
    presets: ['node'],
  })

  const outcome = executeCleanup(plan)

  t.is(outcome.errors.length, 0)
  t.is(outcome.removed.length, 1)
  t.true(outcome.bytesRemoved > 0)
  t.true(outcome.removed[0].path.endsWith('node_modules'))
  t.false(existsSync(plan.entries[0].path))
})

function candidateEndingWith(candidates: CleanupCandidate[], suffix: string) {
  return candidates.find((candidate) => candidate.path.endsWith(suffix))
}
