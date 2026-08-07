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

function mount(height = 20) {
  const styles = new StyleTable()
  const surface = new MemoryCellSurface({ styles })
  const root = createTuiSolidRoot({ surface, styles, size: { width: 100, height } })
  const data = fixture()
  const options: TuiOptions = {
    initialData: data,
    sort: 'path',
    refreshData: () => data,
    executeEntries: (selected) => ({ removed: selected, bytesRemoved: selected.length, errors: [] }),
  }
  root.render(() => <SpaceLensApp options={options} terminalHeight={() => height} onQuit={() => {}} />)
  return { root, surface }
}

const textKey = (text: string) => ({ type: 'text' as const, text })
const namedKey = (key: string) => ({ type: 'key' as const, key, ctrl: false, alt: false, shift: false, meta: false })

describe('Space Lens Uniview Solid TUI', () => {
  it('renders a bounded list with a scrollbar and follows j/k movement', async () => {
    const { root, surface } = mount()
    await tick()
    const initial = surface.text({ trimRight: true })
    expect(initial).toContain('scan tree')
    expect(initial).toContain('8 / 41')
    expect(initial).toContain('│')
    expect(initial).not.toContain('node-39')

    for (let index = 0; index < 20; index += 1) root.dispatchInput(textKey('j'))
    await tick()
    const moved = surface.text({ trimRight: true })
    expect(moved).toContain('node-19')
    expect(moved).toContain('21 / 41')
    expect(moved).not.toContain('node-0')
    root.destroy()
  })

  it('keeps the CLI controls for mode switch, selection, confirmation, and cancel', async () => {
    const { root, surface } = mount()
    await tick()
    root.dispatchInput(namedKey('Tab'))
    await tick()
    expect(surface.text({ trimRight: true })).toContain('CLEAN')

    root.dispatchInput(textKey(' '))
    root.dispatchInput(textKey('x'))
    await tick()
    expect(surface.text({ trimRight: true })).toContain('Confirm delete 1 paths')

    root.dispatchInput(namedKey('Escape'))
    await tick()
    expect(surface.text({ trimRight: true })).not.toContain('Confirm delete')
    root.destroy()
  })
})
