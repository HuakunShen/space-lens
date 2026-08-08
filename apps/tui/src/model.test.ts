import assert from 'node:assert/strict'
import test from 'node:test'

import {
  applyTuiAction,
  createCleanViewModel,
  createInitialTuiState,
  createScanViewModel,
  formatBytes,
  getSelectedEntries,
  type DirectoryNode,
} from './model.js'

test('formatBytes renders binary units without repeating raw byte counts', () => {
  assert.equal(formatBytes(0), '0 B')
  assert.equal(formatBytes(999), '999 B')
  assert.equal(formatBytes(1024), '1.0 KiB')
  assert.equal(formatBytes(1_572_864), '1.5 MiB')
})

test('createCleanViewModel sorts rows by size and summarizes the plan', () => {
  const state = createInitialTuiState()
  const viewModel = createCleanViewModel(
    {
      entries: [
        {
          path: '/repo/node_modules',
          size: 1024,
          preset: 'node',
          reason: 'Node dependency directory',
        },
        {
          path: '/repo/target',
          size: 1_572_864,
          preset: 'rust',
          reason: 'Cargo build output directory',
        },
      ],
      totalSize: 1_573_888,
      errors: ['one unreadable path'],
    },
    { sort: 'size', state },
  )

  assert.equal(viewModel.title, 'Space Lens')
  assert.equal(viewModel.summary, '2 candidates | 1.5 MiB | selected 0 B')
  assert.deepEqual(
    viewModel.rows.map((row) => row.path),
    ['/repo/target', '/repo/node_modules'],
  )
  assert.equal(viewModel.rows[0].sizeLabel, '1.5 MiB')
  assert.equal(viewModel.rows[0].selected, false)
  assert.equal(viewModel.cursor, 0)
  assert.deepEqual(viewModel.errors, ['one unreadable path'])
})

test('createScanViewModel expands a folder and sorts visible children by size', () => {
  const state = createInitialTuiState()
  const trees: DirectoryNode[] = [
    {
      name: 'repo',
      path: '/repo',
      size: 7168,
      depth: 0,
      ignored: false,
      collapsed: false,
      children: [
        {
          name: 'small.txt',
          path: '/repo/small.txt',
          size: 1024,
          depth: 1,
          ignored: false,
          collapsed: false,
          children: [],
        },
        {
          name: 'large.bin',
          path: '/repo/large.bin',
          size: 4096,
          depth: 1,
          ignored: false,
          collapsed: false,
          children: [],
        },
        {
          name: 'cache',
          path: '/repo/cache',
          size: 2048,
          depth: 1,
          ignored: false,
          collapsed: false,
          children: [
            {
              name: 'nested.bin',
              path: '/repo/cache/nested.bin',
              size: 2048,
              depth: 2,
              ignored: false,
              collapsed: false,
              children: [],
            },
          ],
        },
      ],
    },
  ]

  const initial = createScanViewModel(trees, state)
  assert.deepEqual(
    initial.rows.map((row) => row.path),
    ['/repo', '/repo/large.bin', '/repo/cache', '/repo/small.txt'],
  )
  assert.equal(initial.rows[0].expandable, false)
  assert.equal(initial.rows[0].expanded, true)
  assert.equal(initial.rows[2].expandable, true)
  assert.equal(initial.rows[2].expanded, false)

  applyTuiAction(state, { type: 'toggle-expanded', path: '/repo/cache' })
  const expanded = createScanViewModel(trees, state)
  assert.deepEqual(
    expanded.rows.map((row) => row.path),
    ['/repo', '/repo/large.bin', '/repo/cache', '/repo/cache/nested.bin', '/repo/small.txt'],
  )
  assert.equal(expanded.rows[0].sizeLabel, '7.0 KiB')
  assert.equal(expanded.rows[1].sizeLabel, '4.0 KiB')
  assert.equal(expanded.rows[2].expanded, true)
})

test('createScanViewModel marks preset candidates in the complete scan tree', () => {
  const state = createInitialTuiState()
  const viewModel = createScanViewModel(
    [
      {
        name: 'repo',
        path: '/repo',
        size: 4096,
        depth: 0,
        ignored: false,
        collapsed: false,
        children: [
          {
            name: 'target',
            path: '/repo/target',
            size: 4096,
            depth: 1,
            ignored: false,
            collapsed: false,
            children: [],
          },
        ],
      },
    ],
    state,
    new Map([['/repo/target', 'rust']]),
  )

  assert.equal(viewModel.rows[1].preset, 'rust')
  assert.equal(viewModel.rows[1].directory, false)
})

test('applyTuiAction toggles folder expansion state', () => {
  const state = createInitialTuiState()

  applyTuiAction(state, { type: 'toggle-expanded', path: '/repo/cache' })
  assert.deepEqual([...state.expandedScanPaths], ['/repo/cache'])

  applyTuiAction(state, { type: 'toggle-expanded', path: '/repo/cache' })
  assert.deepEqual([...state.expandedScanPaths], [])
})

test('createScanViewModel keeps ignored collapsed leaf rows visible', () => {
  const viewModel = createScanViewModel(
    [
      {
        name: 'repo',
        path: '/repo',
        size: 4096,
        depth: 0,
        ignored: false,
        collapsed: false,
        children: [
          {
            name: 'src',
            path: '/repo/src',
            size: 1024,
            depth: 1,
            ignored: false,
            collapsed: false,
            children: [],
          },
          {
            name: 'target',
            path: '/repo/target',
            size: 3072,
            depth: 1,
            ignored: true,
            collapsed: true,
            children: [],
          },
        ],
      },
    ],
    createInitialTuiState(),
  )

  assert.equal(viewModel.summary, '3 nodes | 4.0 KiB')
  assert.deepEqual(
    viewModel.rows.map((row) => row.label),
    ['▾ repo', '    target [ignored] [collapsed]', '    src'],
  )
  assert.equal(viewModel.rows[1].sizeLabel, '3.0 KiB')
})

test('applyTuiAction switches modes, moves the active cursor, toggles selection, and confirms execute', () => {
  const state = createInitialTuiState()

  applyTuiAction(state, { type: 'switch-mode' })
  assert.equal(state.mode, 'clean')

  applyTuiAction(state, { type: 'move', delta: 1, rowCount: 3 })
  assert.equal(state.cleanCursor, 1)

  applyTuiAction(state, { type: 'toggle-selected', path: '/repo/target' })
  assert.deepEqual([...state.selectedPaths], ['/repo/target'])

  applyTuiAction(state, { type: 'request-execute' })
  assert.equal(state.confirmExecute, true)

  applyTuiAction(state, { type: 'cancel-execute' })
  assert.equal(state.confirmExecute, false)

  applyTuiAction(state, { type: 'switch-mode' })
  assert.equal(state.mode, 'scan')
  applyTuiAction(state, { type: 'move', delta: 2, rowCount: 4 })
  assert.equal(state.scanCursor, 2)
})

test('applyTuiAction toggles all visible clean rows and returns selected entries', () => {
  const state = createInitialTuiState()
  const rows = [
    { path: '/repo/target', size: 2048, preset: 'rust', reason: 'Cargo build output directory' },
    { path: '/repo/node_modules', size: 1024, preset: 'node', reason: 'Node dependency directory' },
  ]

  applyTuiAction(state, { type: 'toggle-all', paths: rows.map((row) => row.path) })
  assert.deepEqual([...state.selectedPaths].sort(), ['/repo/node_modules', '/repo/target'])

  const selected = getSelectedEntries(state, rows)
  assert.deepEqual(
    selected.map((entry) => entry.path),
    ['/repo/target', '/repo/node_modules'],
  )

  applyTuiAction(state, { type: 'toggle-all', paths: rows.map((row) => row.path) })
  assert.deepEqual([...state.selectedPaths], [])
})

test('applyTuiAction requests deletion for the current scan row and all preset candidates', () => {
  const state = createInitialTuiState()

  applyTuiAction(state, {
    type: 'request-delete',
    target: { path: './notes', size: 2048, directory: true },
  })
  assert.deepEqual(state.confirmDelete, { path: './notes', size: 2048, directory: true })

  applyTuiAction(state, { type: 'cancel-delete' })
  assert.equal(state.confirmDelete, undefined)

  applyTuiAction(state, { type: 'request-delete-all' })
  assert.equal(state.confirmDeleteAll, true)
})
