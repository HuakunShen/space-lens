/** Disposable local browser harness. Never invokes the operating system Trash. */
import { mkdtemp, mkdir, writeFile, rename, realpath } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, basename } from 'node:path'
import { startServe } from '../../../packages/host/src/server.ts'
const fixture = await realpath(await mkdtemp(join(tmpdir(), 'spacelens-web-')))
const workspace = join(fixture, 'workspace')
const recovery = join(fixture, 'recovery')
await mkdir(workspace)
await mkdir(recovery)
await writeFile(join(workspace, 'Cargo.toml'), '[package]\nname="fixture"\nversion="0.1.0"\n')
await writeFile(join(workspace, 'package.json'), '{"name":"fixture"}')
await writeFile(join(workspace, '.gitignore'), 'target/\nnode_modules/\n.pytest_cache/\n.ruff_cache/\nignored/\n')
for (const name of ['target', 'node_modules', '__pycache__', '.pytest_cache', '.ruff_cache', 'ignored']) {
  await mkdir(join(workspace, name))
  await writeFile(join(workspace, name, 'generated.bin'), Buffer.alloc(16 * 1024, 1))
}
await mkdir(join(workspace, 'Library/CloudStorage/GoogleDrive-fixture'), { recursive: true })
await writeFile(join(workspace, 'Library/CloudStorage/GoogleDrive-fixture/never-scan.txt'), 'Disposable local fixture, not real cloud storage')
await writeFile(join(workspace, 'large.bin'), Buffer.alloc(12 * 1024 ** 2, 1))
for (let index = 0; index < 205; index++)
  await writeFile(join(workspace, `item-${String(index).padStart(3, '0')}.txt`), Buffer.alloc((index + 1) * 100, 1))
const server = await startServe({
  host: 'loopback',
  port: 9423,
  allowCleanup: true,
  roots: [workspace],
  webRoot: new URL('../build', import.meta.url).pathname,
  ticketTtlSeconds: 600,
  trash: {
    async trash(paths) {
      const results = new Map<string, string | null>()
      for (const path of paths) {
        if (basename(path) === '.ruff_cache') {
          results.set(path, 'Fixture: this cache is in use')
          continue
        }
        try {
          await rename(path, join(recovery, basename(path)))
          results.set(path, null)
        } catch (error) {
          results.set(path, String(error))
        }
      }
      return results
    },
  },
})
console.log(JSON.stringify({ url: server.mintPairingUrl(), workspace, recovery }))
process.on('SIGINT', async () => {
  await server.stop()
  process.exit(0)
})
