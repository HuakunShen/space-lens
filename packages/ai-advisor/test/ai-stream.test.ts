import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createChatStreamParser } from '../src/stream.ts'

// ---------------------------------------------------------------------------
// The one SSE parser every transport shares. Input is arbitrary text chunks of
// an OpenAI-compatible `stream: true` body, so boundaries may fall mid-line or
// mid-JSON; a hostile stream must never throw and never grow past its caps.
// ---------------------------------------------------------------------------

const encoder = new TextEncoder()

function sse(payload: unknown): string {
  return `data: ${typeof payload === 'string' ? payload : JSON.stringify(payload)}\n`
}

/** One assistant text delta in the upstream wire shape. */
function delta(content: string): unknown {
  return { choices: [{ delta: { content }, finish_reason: null }] }
}

test('assembles text split across arbitrary chunk boundaries', () => {
  const deltas: string[] = []
  const parser = createChatStreamParser((text) => deltas.push(text))
  const body = sse(delta('Hello, ')) + sse(delta('world')) + sse('[DONE]')

  for (let index = 0; index < body.length; index += 7) parser.push(body.slice(index, index + 7))

  const response = parser.finish()
  assert.equal(response.choices?.[0]?.message?.content, 'Hello, world')
  assert.deepEqual(deltas, ['Hello, ', 'world'])
  assert.equal(response.choices?.[0]?.message?.tool_calls, undefined)
})

test('accepts \\r\\n line endings, even split between the \\r and the \\n', () => {
  const parser = createChatStreamParser()
  parser.push(`data: ${JSON.stringify(delta('a'))}\r\n`)
  parser.push(`data: ${JSON.stringify(delta('b'))}\r`)
  parser.push('\n')
  parser.push('data: [DONE]\r\n')

  assert.equal(parser.finish().choices?.[0]?.message?.content, 'ab')
})

test('concatenates tool-call fragments and keeps parallel calls in index order', () => {
  const parser = createChatStreamParser()
  parser.push(
    sse({
      choices: [
        {
          delta: {
            tool_calls: [
              { index: 1, id: 'call_b', type: 'function', function: { name: 'list_children', arguments: '{"nodeId":"b' } },
            ],
          },
          finish_reason: null,
        },
      ],
    }),
  )
  parser.push(
    sse({
      choices: [
        {
          delta: {
            tool_calls: [
              { index: 0, id: 'call_a', type: 'function', function: { name: 'get_scan_summary', arguments: '{}' } },
            ],
          },
          finish_reason: null,
        },
      ],
    }),
  )
  parser.push(sse({ choices: [{ delta: { tool_calls: [{ index: 1, function: { arguments: '"}' } }] }, finish_reason: null }] }))
  parser.push(sse('[DONE]'))

  assert.deepEqual(parser.finish().choices?.[0]?.message?.tool_calls, [
    { id: 'call_a', type: 'function', function: { name: 'get_scan_summary', arguments: '{}' } },
    { id: 'call_b', type: 'function', function: { name: 'list_children', arguments: '{"nodeId":"b"}' } },
  ])
})

test('drops a nameless call and names a missing id from its index', () => {
  const parser = createChatStreamParser()
  parser.push(
    sse({
      choices: [
        {
          delta: {
            tool_calls: [
              { index: 2, function: { arguments: '{}' } },
              { index: 3, function: { name: 'find_cache_candidates' } },
            ],
          },
          finish_reason: null,
        },
      ],
    }),
  )

  assert.deepEqual(parser.finish().choices?.[0]?.message?.tool_calls, [
    { id: 'call_3', type: 'function', function: { name: 'find_cache_candidates', arguments: '' } },
  ])
})

test('[DONE] ends the stream and later data is ignored', () => {
  const parser = createChatStreamParser()
  parser.push(sse(delta('kept')))
  parser.push(sse('[DONE]'))
  parser.push(sse(delta('ignored')))

  const response = parser.finish()
  assert.equal(response.choices?.[0]?.message?.content, 'kept')
  assert.equal(JSON.stringify(response).includes('ignored'), false)
})

test('malformed, blank, comment and non-data lines are skipped without throwing', () => {
  const parser = createChatStreamParser()
  parser.push('data: {not json\n')
  parser.push('\n')
  parser.push(': a comment\n')
  parser.push('event: ping\n')
  parser.push('id: 7\n')
  parser.push('data: 42\n')
  parser.push(`data: ${JSON.stringify(delta('after'))}\n`)
  parser.push('data: [DONE]\n')

  assert.equal(parser.finish().choices?.[0]?.message?.content, 'after')
})

test('an error payload is remembered and surfaced by finish()', () => {
  const parser = createChatStreamParser()
  parser.push(sse(delta('partial')))
  parser.push(sse({ error: { message: 'rate limited' } }))
  parser.push(sse('[DONE]'))

  assert.deepEqual(parser.finish(), { error: { message: 'rate limited' } })
})

test('reasoning_content is never shown or stored', () => {
  const deltas: string[] = []
  const parser = createChatStreamParser((text) => deltas.push(text))
  parser.push(sse({ choices: [{ delta: { reasoning_content: 'thinking hard' }, finish_reason: null }] }))
  parser.push(sse({ choices: [{ delta: { reasoning_content: 'more', content: 'answer' }, finish_reason: 'stop' }] }))

  const response = parser.finish()
  assert.equal(response.choices?.[0]?.message?.content, 'answer')
  assert.equal(response.choices?.[0]?.finish_reason, 'stop')
  assert.deepEqual(deltas, ['answer'])
  assert.equal(JSON.stringify(response).includes('thinking'), false)
})

test('caps assembled text at 256 KiB and stops accepting content', () => {
  const deltas: string[] = []
  const parser = createChatStreamParser((text) => deltas.push(text))
  const piece = 'a'.repeat(1024)
  for (let index = 0; index < 400; index += 1) parser.push(sse(delta(piece)))
  parser.push(sse(delta('late')))

  const content = parser.finish().choices?.[0]?.message?.content ?? ''
  assert.equal(encoder.encode(content).length, 256 * 1024)
  assert.equal(deltas.join(''), content)
})

test('caps tool-call arguments at 64 KiB per call', () => {
  const parser = createChatStreamParser()
  const fragment = 'x'.repeat(1024)
  for (let index = 0; index < 80; index += 1) {
    parser.push(
      sse({
        choices: [
          {
            delta: {
              tool_calls: [
                { index: 0, id: 'call_a', type: 'function', function: { name: 'list_children', arguments: fragment } },
              ],
            },
            finish_reason: null,
          },
        ],
      }),
    )
  }

  const args = parser.finish().choices?.[0]?.message?.tool_calls?.[0]?.function.arguments ?? ''
  assert.equal(encoder.encode(args).length, 64 * 1024)
})

test('keeps at most 32 distinct tool calls', () => {
  const parser = createChatStreamParser()
  for (let index = 0; index < 40; index += 1) {
    parser.push(
      sse({
        choices: [
          {
            delta: {
              tool_calls: [
                { index, id: `call_${index}`, type: 'function', function: { name: `tool_${index}`, arguments: '{}' } },
              ],
            },
            finish_reason: null,
          },
        ],
      }),
    )
  }

  const calls = parser.finish().choices?.[0]?.message?.tool_calls ?? []
  assert.equal(calls.length, 32)
  assert.equal(calls[0]?.function.name, 'tool_0')
  assert.equal(calls[31]?.function.name, 'tool_31')
})

test('caps total raw input at 4 MiB and ignores everything after', () => {
  const deltas: string[] = []
  const parser = createChatStreamParser((text) => deltas.push(text))
  parser.push('x'.repeat(4 * 1024 * 1024 + 1))
  parser.push(sse(delta('late')))

  assert.deepEqual(deltas, [])
  assert.deepEqual(parser.finish(), {})
})

test('nothing at all yields an empty response', () => {
  const parser = createChatStreamParser()
  assert.deepEqual(parser.finish(), {})
  parser.push('')
  parser.push('data: [DONE]\n')
  assert.deepEqual(parser.finish(), {})
})
