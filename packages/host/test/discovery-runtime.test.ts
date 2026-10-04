import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const run = promisify(execFile)
const managerModule = new URL('../src/scan-store.ts', import.meta.url).href
const eventsModule = new URL('../src/events.ts', import.meta.url).href
const cwd = fileURLToPath(new URL('..', import.meta.url))

describe('discovery runtime', () => {
  it('runs real-engine discovery through the tsx production fixture loader', async () => {
    const script = `
      import { ScanManager } from ${JSON.stringify(managerModule)}
      import { EventRing } from ${JSON.stringify(eventsModule)}
      import { mkdtempSync, mkdirSync, writeFileSync, realpathSync, rmSync } from 'node:fs'
      import { tmpdir } from 'node:os'
      import { join } from 'node:path'
      const root = realpathSync(mkdtempSync(join(tmpdir(), 'lens-tsx-')))
      try {
        mkdirSync(join(root, 'ignored', 'deep'), { recursive: true })
        writeFileSync(join(root, '.gitignore'), 'ignored/\\n')
        writeFileSync(join(root, 'ignored', 'deep', 'large.bin'), Buffer.alloc(16384, 1))
        const manager = new ScanManager({ events: new EventRing(), trash: { trash: async () => new Map() },
          roots: [root], cleanupMode: 'none', maxConcurrent: 1, maxTotal: 1 })
        const scan = await manager.start({ paths: [root], ignoreHidden: false, respectGitignore: true, ignoredMode: 'summarize' })
        await manager.wait(scan.scanId)
        const page = await manager.discover({ scanId: scan.scanId, kind: 'large-files', minSize: 8192, offset: 0, limit: 1 })
        console.log(JSON.stringify({ total: page.total, name: page.items[0]?.node.name, ignored: page.items[0]?.node.ignored }))
      } finally { rmSync(root, { recursive: true, force: true }) }
    `
    const { stdout } = await run(process.execPath, ['--import', 'tsx', '--input-type=module', '--eval', script], {
      cwd,
    })
    expect(JSON.parse(stdout)).toEqual({ total: 1, name: 'large.bin', ignored: true })
  })
})
