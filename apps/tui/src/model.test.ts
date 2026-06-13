import assert from 'node:assert/strict'
import test from 'node:test'

import {
  applyTuiAction,
  createCleanViewModel,
  createInitialTuiState,
  createScanViewModel,
  formatBytes,
  getSelectedEntries,
} from './model.js'

test('formatBytes renders binary units with raw bytes for non-byte values', () => {
  assert.equal(formatBytes(0), '0 B')
  assert.equal(formatBytes(999), '999 B')
  assert.equal(formatBytes(1024), '1.0 KiB (1024 bytes)')
  assert.equal(formatBytes(1_572_864), '1.5 MiB (1572864 bytes)')
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
  assert.equal(viewModel.summary, '2 candidates | 1.5 MiB (1573888 bytes) | selected 0 B')
  assert.deepEqual(
    viewModel.rows.map((row) => row.path),
    ['/repo/target', '/repo/node_modules'],
  )
  assert.equal(viewModel.rows[0].sizeLabel, '1.5 MiB (1572864 bytes)')
  assert.equal(viewModel.rows[0].selected, false)
  assert.equal(viewModel.cursor, 0)
  assert.deepEqual(viewModel.errors, ['one unreadable path'])
})

test('createScanViewModel flattens directory trees with branch prefixes and sizes', () => {
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

  assert.equal(viewModel.summary, '3 nodes | 4.0 KiB (4096 bytes)')
  assert.deepEqual(
    viewModel.rows.map((row) => row.label),
    ['repo', '  src', '  target [ignored] [collapsed]'],
  )
  assert.equal(viewModel.rows[2].sizeLabel, '3.0 KiB (3072 bytes)')
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
