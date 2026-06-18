import assert from 'node:assert/strict'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'

import { runWebCli } from './web-cli.js'

test('web CLI prints a clickable tokenized kkrpc URL without legacy REST hints', async () => {
  const staticDir = mkdtempSync(join(tmpdir(), 'space-lens-web-cli-'))
  const port = await findFreePort()
  const writes: string[] = []
  const originalWrite = process.stdout.write

  try {
    writeFileSync(join(staticDir, 'index.html'), '<!doctype html><title>Space Lens</title>')
    process.stdout.write = ((chunk: string | Uint8Array) => {
      writes.push(typeof chunk === 'string' ? chunk : Buffer.from(chunk).toString('utf8'))
      return true
    }) as typeof process.stdout.write

    const running = runWebCli([
      'node',
      'spacelens-web',
      '--static-dir',
      staticDir,
      '--port',
      String(port),
    ])

    try {
      assert.ok(running, 'CLI should return the running web server for tests')
      const output = writes.join('')
      const openLine = output.split('\n').find((line) => line.startsWith('Open Space Lens: '))
      assert.ok(openLine, `Expected Open Space Lens line in output:\n${output}`)

      const appUrl = new URL(openLine.slice('Open Space Lens: '.length))
      const rpcUrlRaw = appUrl.searchParams.get('spaceLensRpc')
      assert.ok(rpcUrlRaw, 'Expected app URL to include spaceLensRpc')

      const rpcUrl = new URL(rpcUrlRaw)
      assert.equal(appUrl.origin, `http://127.0.0.1:${port}`)
      assert.equal(rpcUrl.origin, `ws://127.0.0.1:${port}`)
      assert.equal(rpcUrl.pathname, '/rpc')
      assert.ok(rpcUrl.searchParams.get('token'), 'Expected RPC URL to include boot token')
      assert.doesNotMatch(output, /\/api\//)
      assert.doesNotMatch(output, /REST/i)
    } finally {
      running?.server.close()
    }
  } finally {
    process.stdout.write = originalWrite
    rmSync(staticDir, { recursive: true, force: true })
  }
})

function findFreePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer()
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const address = server.address()
      server.close(() => {
        if (typeof address === 'object' && address) {
          resolve(address.port)
        } else {
          reject(new Error('Unable to allocate free port'))
        }
      })
    })
  })
}
