import { chmodSync, mkdtempSync, mkdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it, vi } from 'vitest'
import { ScanManager, normalizeLocalScanPath } from '../src/scan-store.ts'
import { EventRing } from '../src/events.ts'
import { EventEnvelopeSchema } from '@space-lens/contract'
import { LOCAL_SCAN_BIN_ENV, resolveLocalScanBin } from '../src/local-process.ts'

const metadata = vi.hoisted(() => ({ paths: [] as string[] }))
vi.mock('node:fs', async (importOriginal) => {
  const actual = await importOriginal<typeof import('node:fs')>()
  return {
    ...actual,
    lstatSync: (...args: Parameters<typeof actual.lstatSync>) => {
      metadata.paths.push(String(args[0]))
      return actual.lstatSync(...args)
    },
    realpathSync: (...args: Parameters<typeof actual.realpathSync>) => {
      metadata.paths.push(String(args[0]))
      return actual.realpathSync(...args)
    },
  }
})

function fixture(scenario = 'ok', base = tmpdir()) {
  const temp = realpathSync(mkdtempSync(join(base, 'lens-local-host-')))
  const root = join(temp, 'local fixture')
  mkdirSync(join(root, 'project', 'node_modules'), { recursive: true })
  writeFileSync(join(root, 'project', 'package.json'), '{}')
  writeFileSync(join(root, 'project', 'node_modules', 'large.bin'), Buffer.alloc(16384, 1))
  if (scenario === 'stream-cloud') mkdirSync(join(root, 'Library', 'CloudStorage'), { recursive: true })
  if (scenario === 'large-report' || scenario === 'stream') {
    for (let index = 0; index < 1500; index += 1) writeFileSync(join(root, `entry-${index}.bin`), 'x')
  }
  const binary = join(temp, 'protected-cli')
  const source = `#!${process.execPath}
const fs = require('node:fs')
const path = require('node:path')
const args = process.argv.slice(2)
if (args[0] !== 'scan' || !args.includes('--local-only') || !args.includes('--json') || !args.includes('--progress-json')) process.exit(12)
if (!args.includes('--respect-gitignore') || !args.includes('--ignored-mode')) process.exit(13)
const root = ${JSON.stringify(scenario)} === 'canonical-report' ? fs.realpathSync(args[1]) : args[1]
const scenario = ${JSON.stringify(scenario)}
fs.writeFileSync(${JSON.stringify(binary + '.pid')}, String(process.pid))
if (scenario === 'fail') { process.stderr.write('denied diagnostic\\n'.repeat(10000)); process.exit(7) }
if (scenario === 'malformed') { process.stdout.write('{invalid\\n'); setInterval(() => {}, 1000); return }
if (scenario === 'hang') { process.stdout.write(JSON.stringify({type:'progress',progress:{currentPath:root,bytesScanned:0,entriesScanned:0,files:0,directories:0,skippedCount:0,deniedCount:0,elapsedMs:1}})+'\\n'); setInterval(() => {},1000); return }
let fileCount = 0, directoryCount = 0
const walk = (value, depth = 0) => {
  const stat = fs.lstatSync(value)
  if(stat.isDirectory()) directoryCount++; else fileCount++
  const children = stat.isDirectory() ? fs.readdirSync(value).map((name) => walk(path.join(value,name),depth+1)) : []
  return {name:path.basename(value),path:value,size:stat.blocks*512+children.reduce((sum,n)=>sum+n.size,0),
    logicalSize:stat.isDirectory()?children.reduce((sum,n)=>sum+n.logicalSize,0):stat.size,
    children,depth,ignored:false,collapsed:false,isDirectory:stat.isDirectory(),scanState:'complete',skipReason:null}
}
const node = walk(root)
if(scenario==='classified') {
  const ignored = node.children.find(n=>n.name==='project').children.find(n=>n.name==='node_modules')
  const mark = (node) => { node.ignored = true; for(const child of node.children) mark(child) }
  mark(ignored)
}
node.children.push({name:'cloud-placeholder',path:path.join(root,'cloud-placeholder'),size:0,logicalSize:99999,
  children:[],depth:1,ignored:false,collapsed:true,isDirectory:true,scanState:'skipped',skipReason:'dataless'})
node.scanState='partial'
const coverage = {mode:'local-only',protection:process.platform==='darwin'?'macos-no-materialization':'metadata-only',sizeMetric:'allocated',logicalBytes:node.logicalSize,
  files:fileCount,directories:directoryCount,skippedCount:1,deniedCount:0,issueCount:1,issuesTruncated:false,issues:[{path:path.join(root,'cloud-placeholder'),reason:'dataless',message:'not materialized'}],elapsedMs:20}
const report = {nodes:[node],coverage,volumes:[{path:root,totalBytes:1000000,availableBytes:400000,freeBytes:500000,isLocal:true}]}
if(scenario==='nested-roots') {
  const selected = node.children.find(child=>child.path===path.join(root,'project'))
  node.children = node.children.filter(child=>child!==selected)
  node.size -= selected.size
  node.logicalSize -= selected.logicalSize
  report.nodes.push(selected)
}
if(scenario==='wrong-protection') coverage.protection = 'metadata-only'
if(scenario==='unsafe-report') node.children[0].path = '/outside/never-probe'
const progress = {type:'progress',progress:{currentPath:path.join(root,'中文'),bytesScanned:16384,entriesScanned:5,files:2,directories:3,skippedCount:1,deniedCount:0,elapsedMs:10}}
const bytes = Buffer.from(JSON.stringify(progress)+'\\n')
const cut = bytes.indexOf(Buffer.from('中文')) + 1
process.stdout.write(bytes.subarray(0,cut))
setTimeout(() => {
  process.stdout.write(bytes.subarray(cut))
  for(let i=0;i<100;i++) process.stdout.write(JSON.stringify(progress)+'\\n')
  if(scenario.startsWith('stream')) {
    if(!args.includes('--stream-nodes')) { process.stderr.write('missing --stream-nodes'); process.exitCode=14; return }
    const frames = []
    const emit = (node) => { frames.push({...node,children:[]}); for(const child of node.children) emit(child) }
    for(const node of report.nodes) emit(node)
    if(scenario==='stream-duplicate') frames.push(frames[0])
    if(scenario==='stream-child-first') [frames[0],frames[1]]=[frames[1],frames[0]]
    if(scenario==='stream-depth') frames[1].depth = 9
    if(scenario==='stream-outside') frames[1].path = '/outside/never-probe'
    if(scenario==='stream-skipped-parent') { frames[0].scanState='skipped'; frames[0].skipReason='dataless' }
    if(scenario==='stream-closed-parent') frames.push(frames.splice(frames.findIndex(node=>node.name==='large.bin'),1)[0])
    if(scenario==='stream-huge-node') frames[0].name = 'x'.repeat(1024*1024)
    for(const node of frames) process.stdout.write(JSON.stringify({type:'node',node})+'\\n')
    report.nodes=[]
    if(scenario==='stream-hang') { setInterval(()=>{},1000); return }
    if(scenario==='stream-no-done') return
    if(scenario==='stream-bad-exit') process.exitCode=3
  }
  process.stdout.write(JSON.stringify({type:'done',report})+'\\n')
  if(scenario==='bad-exit') process.exitCode=3
  if(scenario==='no-done') { /* report removal handled below */ }
}, 10)
`
  writeFileSync(
    binary,
    source.replace(
      scenario === 'no-done' ? "process.stdout.write(JSON.stringify({type:'done',report})+'\\n')" : '__unused__',
      '',
    ),
  )
  chmodSync(binary, 0o755)
  const events = new EventRing()
  const manager = new ScanManager({
    events,
    trash: {
      trash: async () => {
        throw new Error('read-only must not trash')
      },
    },
    roots: [root],
    cleanupMode: 'trash',
    maxConcurrent: 1,
    maxTotal: 3,
    localScanBin: binary,
  })
  const request = {
    paths: [root],
    ignoreHidden: false,
    respectGitignore: true,
    ignoredMode: 'summarize' as const,
    localOnly: true,
  }
  const cleanup = async () => {
    await manager.close()
    rmSync(temp, { recursive: true, force: true })
  }
  return { temp, root, binary, events, manager, request, cleanup }
}

describe('protected CLI scan', () => {
  it('assembles streamed real fixture nodes and indexes their children after done and successful exit', async () => {
    const setup = fixture('stream')
    try {
      metadata.paths = []
      const scan = await setup.manager.start(setup.request, undefined)
      await setup.manager.wait(scan.scanId)
      const status = setup.manager.status(scan.scanId)
      expect(status.state).toBe('ready')
      expect(status.coverage?.files).toBe(1502)
      const page = setup.manager.children(scan.scanId, status.rootIds[0], 0, 1000, 'name')
      expect(page.total).toBe(1502)
      expect(page.items).toHaveLength(1000)
      expect(metadata.paths).toEqual([])
    } finally {
      await setup.cleanup()
    }
  })
  it.each([
    'stream-duplicate',
    'stream-child-first',
    'stream-depth',
    'stream-outside',
    'stream-skipped-parent',
    'stream-closed-parent',
    'stream-cloud',
    'stream-huge-node',
    'stream-no-done',
    'stream-bad-exit',
  ])('fails closed for %s protocol', async (scenario) => {
    const setup = fixture(scenario)
    try {
      const scan = await setup.manager.start(setup.request, undefined)
      await setup.manager.wait(scan.scanId)
      expect(setup.manager.status(scan.scanId).state).toBe('failed')
      expect(setup.manager.status(scan.scanId).rootIds).toEqual([])
      expect(setup.events.replay(0).events.some((event) => event.payload.kind === 'scan.completed')).toBe(false)
    } finally {
      await setup.cleanup()
    }
  })
  it('discards partially streamed nodes when the running process is cancelled', async () => {
    const setup = fixture('stream-hang')
    try {
      const scan = await setup.manager.start(setup.request, undefined)
      const deadline = Date.now() + 5000
      while (setup.manager.status(scan.scanId).currentPath === null && Date.now() < deadline)
        await new Promise((resolve) => setTimeout(resolve, 5))
      expect(setup.manager.status(scan.scanId).state).toBe('scanning')
      const pid = Number(readFileSync(setup.binary + '.pid', 'utf8'))
      setup.manager.cancel(scan.scanId)
      await setup.manager.wait(scan.scanId)
      expect(setup.manager.status(scan.scanId).rootIds).toEqual([])
      expect(setup.manager.status(scan.scanId).state).toBe('cancelled')
      expect(() => process.kill(pid, 0)).toThrow()
    } finally {
      await setup.cleanup()
    }
  })
  it.runIf(process.platform === 'darwin')(
    'normalizes protected Darwin aliases and served roots without metadata probes',
    async () => {
      const setup = fixture('canonical-report', '/tmp')
      const alias = setup.root.replace(/^\/private\/tmp(?=\/)/, '/tmp')
      const manager = new ScanManager({
        events: setup.events,
        trash: { trash: async () => new Map() },
        roots: [alias],
        cleanupMode: 'none',
        maxConcurrent: 1,
        maxTotal: 1,
        localScanBin: setup.binary,
      })
      try {
        metadata.paths = []
        const scan = await manager.start({ ...setup.request, paths: [alias] }, undefined)
        await manager.wait(scan.scanId)
        const status = manager.status(scan.scanId)
        expect(status.state).toBe('ready')
        expect(manager.slice(scan.scanId, status.rootIds[0], 1, 10).focusNode.path).toBe(setup.root)
        expect(metadata.paths).toEqual([])
        expect(normalizeLocalScanPath('/var/fixture')).toBe('/private/var/fixture')
        expect(normalizeLocalScanPath('/etc/fixture')).toBe('/private/etc/fixture')
        expect(normalizeLocalScanPath('/tmp-other/fixture')).toBe('/tmp-other/fixture')
      } finally {
        await manager.close()
        await setup.cleanup()
      }
    },
  )
  it('consumes a multi-chunk report of real fixture entries and serves child pagination', async () => {
    const setup = fixture('large-report')
    try {
      const scan = await setup.manager.start(setup.request, undefined)
      await setup.manager.wait(scan.scanId)
      const status = setup.manager.status(scan.scanId)
      expect(status.state).toBe('ready')
      expect(status.coverage?.files).toBe(1502)
      const first = setup.manager.children(scan.scanId, status.rootIds[0], 0, 1000, 'name')
      const second = setup.manager.children(scan.scanId, status.rootIds[0], 1000, 1000, 'name')
      expect(first.total).toBe(1502)
      expect(first.items).toHaveLength(1000)
      expect(second.items).toHaveLength(502)
      expect(new Set([...first.items, ...second.items].map((node) => node.id)).size).toBe(1502)
      expect(first.items.every((node) => node.scanState !== undefined)).toBe(true)
    } finally {
      await setup.cleanup()
    }
  })
  it('keeps explicitly selected nested roots as separate protected scan roots', async () => {
    const setup = fixture('nested-roots')
    try {
      const nested = join(setup.root, 'project')
      const scan = await setup.manager.start({ ...setup.request, paths: [setup.root, nested, nested] }, undefined)
      await setup.manager.wait(scan.scanId)
      const status = setup.manager.status(scan.scanId)
      expect(status.state).toBe('ready')
      expect(status.rootIds).toHaveLength(2)
      expect(status.rootIds.map((id) => setup.manager.slice(scan.scanId, id, 1, 10).focusNode.path)).toEqual([
        setup.root,
        nested,
      ])
    } finally {
      await setup.cleanup()
    }
  })
  it('discovers only top-level ignored report nodes without a second scan', async () => {
    const setup = fixture('classified')
    try {
      const scan = await setup.manager.start(setup.request, undefined)
      await setup.manager.wait(scan.scanId)
      metadata.paths = []
      const result = await setup.manager.discover({
        scanId: scan.scanId,
        kind: 'gitignored',
        minSize: 0,
        offset: 0,
        limit: 10,
      })
      expect(result.items.map((item) => item.node.name)).toEqual(['node_modules'])
      expect(result.items[0].category).toBe('Gitignored')
      const files = await setup.manager.discover({
        scanId: scan.scanId,
        kind: 'large-files',
        minSize: 8192,
        offset: 0,
        limit: 10,
      })
      expect(files.items[0].node.ignored).toBe(true)
      expect(metadata.paths).toEqual([])
      const unclassified = await setup.manager.start({ ...setup.request, respectGitignore: false }, undefined)
      await setup.manager.wait(unclassified.scanId)
      await expect(
        setup.manager.discover({ scanId: unclassified.scanId, kind: 'gitignored', minSize: 0, offset: 0, limit: 10 }),
      ).rejects.toMatchObject({ problem: { code: 'UnsupportedOperation' } })
    } finally {
      await setup.cleanup()
    }
  })
  it.runIf(process.platform === 'darwin')('refuses a report without the required macOS protection', async () => {
    const setup = fixture('wrong-protection')
    try {
      const scan = await setup.manager.start(setup.request, undefined)
      await setup.manager.wait(scan.scanId)
      expect(setup.manager.status(scan.scanId).state).toBe('failed')
    } finally {
      await setup.cleanup()
    }
  })
  it('indexes a real local fixture report, exposes bounded progress/coverage, and never probes reported paths', async () => {
    const setup = fixture()
    try {
      metadata.paths = []
      const scan = await setup.manager.start(setup.request, undefined)
      await setup.manager.wait(scan.scanId)
      const status = setup.manager.status(scan.scanId)
      expect(status.state).toBe('ready')
      expect(status.coverage).toMatchObject({ mode: 'local-only', skippedCount: 1, files: 2, logicalBytes: 16386 })
      expect(status.volumes?.[0]).toMatchObject({ totalBytes: 1000000, availableBytes: 400000, freeBytes: 500000 })
      expect(status.bytesScanned).toBeGreaterThanOrEqual(16384)
      const slice = setup.manager.slice(scan.scanId, status.rootIds[0], 8, 100)
      expect(slice.focusNode.scanState).toBe('partial')
      expect(slice.tree.children.find((node) => node.name === 'cloud-placeholder')).toMatchObject({
        scanState: 'skipped',
        skipReason: 'dataless',
        logicalSize: 99999,
      })
      const files = await setup.manager.discover({
        scanId: scan.scanId,
        kind: 'large-files',
        minSize: 8192,
        offset: 0,
        limit: 10,
      })
      expect(files.items.map((item) => item.node.name)).toEqual(['large.bin'])
      const caches = await setup.manager.discover({
        scanId: scan.scanId,
        kind: 'caches',
        minSize: 0,
        offset: 0,
        limit: 10,
      })
      expect(caches.items.map((item) => item.node.name)).toEqual(['node_modules'])
      expect(
        (await setup.manager.discover({ scanId: scan.scanId, kind: 'gitignored', minSize: 0, offset: 0, limit: 10 }))
          .total,
      ).toBe(0)
      expect(() => setup.manager.plan(scan.scanId, [files.items[0].node.id])).toThrow(/read-only/)
      expect(metadata.paths.filter((path) => path === setup.root || path.startsWith(setup.root + '/'))).toEqual([])
      const events = setup.events.replay(0).events
      expect(events.filter((event) => event.payload.kind === 'scan.updated')).toHaveLength(2)
      expect(events.filter((event) => event.payload.kind === 'scan.completed')).toHaveLength(1)
      expect(events.every((event) => EventEnvelopeSchema.safeParse(event).success)).toBe(true)
      expect(
        events.find(
          (event) => event.payload.kind === 'scan.updated' && event.payload.status.currentPath?.endsWith('中文'),
        ),
      ).toBeDefined()
    } finally {
      await setup.cleanup()
    }
  })

  it.each(['fail', 'malformed', 'bad-exit', 'no-done', 'unsafe-report'])(
    'fails closed for CLI %s and never falls back to the native scanner',
    async (scenario) => {
      const setup = fixture(scenario)
      try {
        const scan = await setup.manager.start(setup.request, undefined)
        await setup.manager.wait(scan.scanId)
        const status = setup.manager.status(scan.scanId)
        expect(status.state).toBe('failed')
        expect(status.rootIds).toEqual([])
        expect(status.message.length).toBeLessThan(10000)
        expect(setup.events.replay(0).events.filter((event) => event.payload.kind === 'scan.completed')).toEqual([])
      } finally {
        await setup.cleanup()
      }
    },
  )

  it('uses the explicit environment binary and rejects cloud-root requests before metadata access', async () => {
    const setup = fixture()
    const previous = process.env[LOCAL_SCAN_BIN_ENV]
    try {
      process.env[LOCAL_SCAN_BIN_ENV] = setup.binary
      expect(resolveLocalScanBin()).toBe(setup.binary)
      const manager = new ScanManager({
        events: setup.events,
        trash: { trash: async () => new Map() },
        roots: [homedir()],
        cleanupMode: 'none',
        maxConcurrent: 1,
        maxTotal: 1,
      })
      metadata.paths = []
      await expect(
        manager.start(
          { ...setup.request, paths: [join(homedir(), 'Library', 'CloudStorage', 'do-not-probe')] },
          undefined,
        ),
      ).rejects.toMatchObject({ problem: { code: 'Forbidden' } })
      expect(metadata.paths).toEqual([])
      await manager.close()
    } finally {
      if (previous === undefined) delete process.env[LOCAL_SCAN_BIN_ENV]
      else process.env[LOCAL_SCAN_BIN_ENV] = previous
      await setup.cleanup()
    }
  })

  it('settles process startup errors and releases the native concurrency slot', async () => {
    const setup = fixture()
    try {
      const program = readFileSync(setup.binary)
      rmSync(setup.binary)
      mkdirSync(setup.binary)
      const failed = await setup.manager.start(setup.request, undefined)
      await setup.manager.wait(failed.scanId)
      expect(setup.manager.status(failed.scanId).state).toBe('failed')
      rmSync(setup.binary, { recursive: true })
      writeFileSync(setup.binary, program)
      chmodSync(setup.binary, 0o755)
      const recovered = await setup.manager.start(setup.request, undefined)
      await setup.manager.wait(recovered.scanId)
      expect(setup.manager.status(recovered.scanId).state).toBe('ready')
    } finally {
      await setup.cleanup()
    }
  })

  it('refuses a missing explicit binary without a legacy fallback', async () => {
    const setup = fixture()
    try {
      rmSync(setup.binary)
      await expect(setup.manager.start(setup.request, undefined)).rejects.toMatchObject({
        problem: { code: 'Unavailable' },
      })
      expect(setup.manager.list()).toEqual([])
    } finally {
      await setup.cleanup()
    }
  })

  it('counts protected processes against native concurrency and terminates on cancel/shutdown', async () => {
    const setup = fixture('hang')
    try {
      const scan = await setup.manager.start(setup.request, undefined)
      const deadline = Date.now() + 5000
      while (setup.manager.status(scan.scanId).currentPath === null && Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, 5))
      }
      const pid = Number(readFileSync(setup.binary + '.pid', 'utf8'))
      await expect(setup.manager.start({ ...setup.request, localOnly: false }, undefined)).rejects.toMatchObject({
        problem: { code: 'LimitExceeded' },
      })
      expect(setup.manager.cancel(scan.scanId).state).toBe('cancelled')
      await setup.manager.wait(scan.scanId)
      expect(() => process.kill(pid, 0)).toThrow()
      const another = await setup.manager.start(setup.request, undefined)
      await setup.manager.close()
      await setup.manager.wait(another.scanId)
      expect(setup.manager.status(another.scanId).state).toBe('cancelled')
      expect(setup.events.replay(0).events.filter((event) => event.payload.kind === 'scan.completed')).toEqual([])
    } finally {
      await setup.cleanup()
    }
  })
})
