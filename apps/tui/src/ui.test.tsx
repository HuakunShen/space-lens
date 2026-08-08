import { describe, expect, it } from 'vitest'
import { createTuiSolidRoot, MemoryCellSurface, StyleTable } from '@uniview/tui-solid'
import type { DirectoryNode, PlanEntry } from './model.js'
import type { SpaceLensData } from './scanner.js'
import { SpaceLensApp, type TuiOptions } from './ui'

const tick = async () => {
  for (let index = 0; index < 8; index += 1) await new Promise<void>((resolve) => setTimeout(resolve, 0))
}

function fixture(): SpaceLensData {
  const entries: PlanEntry[] = Array.from({ length: 40 }, (_, index) => ({
    path: `node-${index}`,
    size: index + 1,
    preset: 'node',
    reason: 'dependency cache',
  }))
  const children: DirectoryNode[] = entries.map((entry) => ({
    name: entry.path,
    path: entry.path,
    size: entry.size,
    children: [],
    depth: 1,
    ignored: false,
    collapsed: false,
  }))
  return {
    scanTrees: [
      {
        name: '.',
        path: '.',
        size: entries.reduce((total, entry) => total + entry.size, 0),
        children,
        depth: 0,
        ignored: false,
        collapsed: false,
      },
    ],
    plan: {
      entries,
      totalSize: entries.reduce((total, entry) => total + entry.size, 0),
      errors: [],
    },
  }
}

function nestedFixture(): SpaceLensData {
  const data = fixture()
  const root = data.scanTrees[0]
  const folder = root.children.find((child) => child.name === 'node-39')
  if (!folder) throw new Error('fixture folder missing')

  root.children = root.children.map((child) =>
    child === folder
      ? {
          ...child,
          name: 'folder',
          path: 'folder',
          children: [
            {
              ...child,
              name: 'inside.txt',
              path: 'folder/inside.txt',
              depth: 2,
            },
          ],
        }
      : child,
  )
  return data
}

function mount(height = 20, data = fixture()) {
  const styles = new StyleTable()
  const surface = new MemoryCellSurface({ styles })
  const root = createTuiSolidRoot({ surface, styles, size: { width: 100, height } })
  let tabHandler: (() => void) | undefined
  const options: TuiOptions = {
    initialData: data,
    sort: 'path',
    refreshData: () => data,
    executeEntries: (selected) => ({ removed: selected, bytesRemoved: selected.length, errors: [] }),
    deletePath: (target) => ({ path: target.path, bytesRemoved: target.size }),
  }
  root.render(() => (
    <SpaceLensApp
      options={options}
      terminalHeight={() => height}
      onQuit={() => {}}
      registerTabHandler={(handler) => {
        tabHandler = handler
      }}
    />
  ))
  const dispatch = (event: Parameters<typeof root.dispatchInput>[0]) => {
    if (event.type === 'key' && event.key === 'Tab' && tabHandler) {
      tabHandler()
      return
    }
    root.dispatchInput(event)
  }
  return { root, surface, dispatch }
}

const textKey = (text: string) => ({ type: 'text' as const, text })
const namedKey = (key: string) => ({ type: 'key' as const, key, ctrl: false, alt: false, shift: false, meta: false })

describe('Space Lens Uniview Solid TUI', () => {
  it('renders a bounded list with a scrollbar and follows j/k movement', async () => {
    const { root, surface, dispatch } = mount()
    try {
      await tick()
      const initial = surface.text({ trimRight: true })
      expect(initial).toContain('scan tree')
      expect(initial).toContain('8 / 41')
      expect(initial).toContain('│')
      expect(initial).toContain('node-39')
      expect(initial).not.toContain('node-0')

      for (let index = 0; index < 20; index += 1) dispatch(textKey('j'))
      await tick()
      const moved = surface.text({ trimRight: true })
      expect(moved).toContain('node-20')
      expect(moved).toContain('21 / 41')
      expect(moved).not.toContain('node-0')
    } finally {
      root.destroy()
    }
  })

  it('keeps the top-level folder expanded when Enter is pressed', async () => {
    const { root, surface, dispatch } = mount()
    try {
      await tick()

      const initial = surface.text({ trimRight: true })
      expect(initial).toContain('▾ .')
      expect(initial).toContain('node-39')

      dispatch(namedKey('Enter'))
      await tick()
      expect(surface.text({ trimRight: true })).toContain('node-39')
    } finally {
      root.destroy()
    }
  })

  it('expands a nested folder when its row is clicked', async () => {
    const { root, surface, dispatch } = mount(20, nestedFixture())
    try {
      await tick()
      const initial = surface.text({ trimRight: true })
      expect(initial).toContain('▸ folder')
      expect(initial).not.toContain('inside.txt')

      dispatch({ type: 'mouse', action: 'up', button: 'left', x: 24, y: 9, ctrl: false, alt: false, shift: false })
      await tick()
      expect(surface.text({ trimRight: true })).toContain('inside.txt')
    } finally {
      root.destroy()
    }
  })

  it('keeps the CLI controls for mode switch, selection, confirmation, and cancel', async () => {
    const { root, surface, dispatch } = mount()
    try {
      await tick()
      dispatch(namedKey('Tab'))
      await tick()
      expect(surface.text({ trimRight: true })).toContain('CLEAN')

      dispatch(textKey(' '))
      dispatch(textKey('x'))
      await tick()
      expect(surface.text({ trimRight: true })).toContain('Confirm delete 1 paths')

      dispatch(namedKey('Escape'))
      await tick()
      expect(surface.text({ trimRight: true })).not.toContain('Confirm delete')
    } finally {
      root.destroy()
    }
  })

  it('opens a confirmation dialog for an arbitrary scan row with d', async () => {
    const { root, surface, dispatch } = mount(20, nestedFixture())
    try {
      await tick()
      dispatch(textKey('j'))
      dispatch(textKey('d'))
      await tick()
      expect(surface.text({ trimRight: true })).toContain('Confirm delete')
      expect(surface.text({ trimRight: true })).toContain('folder')
      expect(surface.text({ trimRight: true })).toContain('Enter confirm')
    } finally {
      root.destroy()
    }
  })

  it('opens a batch confirmation dialog for all preset candidates with A', async () => {
    const { root, surface, dispatch } = mount()
    try {
      await tick()
      dispatch(textKey('A'))
      await tick()
      expect(surface.text({ trimRight: true })).toContain('Delete all preset candidates')
    } finally {
      root.destroy()
    }
  })
})
