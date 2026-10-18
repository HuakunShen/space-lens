// Fake `spacelens memory stdio` child for packages/host/test/memory-bridge.test.ts.
//
// It speaks the §4.6 line protocol of docs/memory-cleaner-plan.md: one `hello`
// line first, then exactly one correlated reply per request line. The scenario
// is selected with FAKE_MEMORY_MODE so a single script covers the happy path,
// problem replies, a silent call (timeout), an oversized reply line and a
// premature exit. It asserts the subcommand it was handed so the test also
// proves the bridge's spawn arguments.

import process from 'node:process'

const mode = process.env.FAKE_MEMORY_MODE ?? 'echo'
const argv = process.argv.slice(2)

// The bridge must spawn `<binary> memory stdio`; the fixture is the leading
// `args` entry, so `memory stdio` are the first two subcommand tokens.
if (argv[0] !== 'memory' || argv[1] !== 'stdio') process.exit(64)
if (mode === 'exit-before-hello') process.exit(3)

const HELLO = { protocol: 1, platform: 'macos', terminate: true, gracefulAppQuit: true }

if (mode === 'hello-invalid') {
  process.stdout.write('{"hello":{"protocol":1}}\n')
} else {
  process.stdout.write(`${JSON.stringify({ hello: HELLO })}\n`)
}

/** Write one reply line. */
function reply(value) {
  process.stdout.write(`${JSON.stringify(value)}\n`)
}

/** Handle one decoded request line. */
function handle(request) {
  const method = request?.call?.method
  if (method === 'shutdown') process.exit(0)
  if (mode === 'exit') process.exit(0)
  if (mode === 'timeout') return
  if (mode === 'overflow') {
    // A single reply far larger than the 4 MiB protocol cap, without a newline.
    process.stdout.write('x'.repeat(4 * 1024 * 1024 + 64))
    return
  }
  if (mode === 'problem') {
    reply({
      id: request.id,
      ok: false,
      problem: { code: 'LimitExceeded', message: 'The memory service reached a limit.', retryable: true },
    })
    return
  }
  reply({ id: request.id, ok: true, result: { method, sessionId: request.sessionId, request: request.call.request ?? null } })
}

let buffer = ''
process.stdin.setEncoding('utf8')
process.stdin.on('data', (chunk) => {
  buffer += chunk
  for (;;) {
    const newline = buffer.indexOf('\n')
    if (newline === -1) return
    const line = buffer.slice(0, newline)
    buffer = buffer.slice(newline + 1)
    if (line.trim() !== '') handle(JSON.parse(line))
  }
})
process.stdin.on('end', () => process.exit(0))
