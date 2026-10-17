import type { ChatResponse, ChatToolCall } from './types.ts'

/**
 * Streaming chat core: the one tolerant SSE parser every transport shares.
 *
 * The Tauri channel, the HTTP host and the VS Code webview all forward the raw
 * text chunks of an OpenAI-compatible `stream: true` body; the deltas are
 * decoded exactly once, here. A hostile or broken stream must never throw and
 * must never grow past the caps below, so each field is length-checked while it
 * is assembled and malformed lines are simply dropped. Tool-call arguments stay
 * raw strings: the agent already tolerates malformed JSON, so this layer never
 * parses them.
 */

const MAX_TEXT_BYTES = 256 * 1024
const MAX_ARGUMENT_BYTES = 64 * 1024
const MAX_TOOL_CALLS = 32
const MAX_RAW_BYTES = 4 * 1024 * 1024

const encoder = new TextEncoder()
const decoder = new TextDecoder()

/** UTF-8 byte length, matching the advisor's byte accounting in `tools.ts`. */
function byteLength(text: string): number {
  return encoder.encode(text).length
}

/** Largest prefix of `text` fitting `maxBytes`, never splitting a code point. */
function sliceUtf8(text: string, maxBytes: number): string {
  const encoded = encoder.encode(text)
  if (encoded.length <= maxBytes) return text
  let end = maxBytes
  while (end > 0 && (encoded[end] & 0xc0) === 0x80) end -= 1
  return decoder.decode(encoded.subarray(0, end))
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : null
}

interface PartialCall {
  id: string
  name: string
  args: string
  argsBytes: number
  argsCapped: boolean
}

export interface ChatStreamParser {
  /** Feed one raw text chunk of the upstream body. Never throws. */
  push(chunk: string): void
  /** The fully assembled completion, from whatever arrived. */
  finish(): ChatResponse
}

/** Accumulates a `stream: true` SSE body into one `ChatResponse`. */
export function createChatStreamParser(onText?: (delta: string) => void): ChatStreamParser {
  const calls = new Map<number, PartialCall>()
  /** Bytes never exceed the caps, so plain strings are safe here. */
  let buffer = ''
  let text = ''
  let textBytes = 0
  let textCapped = false
  let finishReason: string | null | undefined
  let errorMessage: string | undefined
  let done = false
  let rawBytes = 0
  let rawCapped = false

  const addText = (delta: string): void => {
    if (textCapped) return
    let piece = delta
    let bytes = byteLength(delta)
    const room = MAX_TEXT_BYTES - textBytes
    if (bytes > room) {
      piece = sliceUtf8(delta, room)
      bytes = byteLength(piece)
      textCapped = true
    }
    if (bytes === 0) return
    text += piece
    textBytes += bytes
    onText?.(piece)
  }

  const addArguments = (call: PartialCall, fragment: string): void => {
    if (call.argsCapped) return
    let piece = fragment
    let bytes = byteLength(fragment)
    const room = MAX_ARGUMENT_BYTES - call.argsBytes
    if (bytes > room) {
      piece = sliceUtf8(fragment, room)
      bytes = byteLength(piece)
      call.argsCapped = true
    }
    if (bytes === 0) return
    call.args += piece
    call.argsBytes += bytes
  }

  const mergeToolCall = (value: unknown): void => {
    const entry = asRecord(value)
    if (entry === null) return
    const rawIndex = entry.index
    const index = typeof rawIndex === 'number' && Number.isFinite(rawIndex) ? rawIndex : 0
    let call = calls.get(index)
    if (call === undefined) {
      if (calls.size >= MAX_TOOL_CALLS) return
      call = { id: '', name: '', args: '', argsBytes: 0, argsCapped: false }
      calls.set(index, call)
    }
    // Identity is write-once: the first non-empty id/name wins.
    if (call.id === '' && typeof entry.id === 'string' && entry.id !== '') call.id = entry.id
    const fn = asRecord(entry.function)
    if (fn === null) return
    if (call.name === '' && typeof fn.name === 'string' && fn.name !== '') call.name = fn.name
    if (typeof fn.arguments === 'string' && fn.arguments !== '') addArguments(call, fn.arguments)
  }

  const handlePayload = (payload: string): void => {
    const body = payload.trim()
    if (body === '[DONE]') {
      done = true
      return
    }
    let parsed: unknown
    try {
      parsed = JSON.parse(body)
    } catch {
      return
    }
    const record = asRecord(parsed)
    if (record === null) return

    const error = asRecord(record.error)
    if (error !== null && typeof error.message === 'string' && error.message !== '') errorMessage = error.message

    if (!Array.isArray(record.choices)) return
    const choice = asRecord(record.choices[0])
    if (choice === null) return
    if (typeof choice.finish_reason === 'string') finishReason = choice.finish_reason
    else if (choice.finish_reason === null) finishReason ??= null

    const delta = asRecord(choice.delta)
    if (delta === null) return
    // `reasoning_content` is deliberately unread: it is never shown or stored.
    if (typeof delta.content === 'string' && delta.content !== '') addText(delta.content)
    if (Array.isArray(delta.tool_calls)) for (const entry of delta.tool_calls) mergeToolCall(entry)
  }

  const handleLine = (line: string): void => {
    if (done || line === '' || line.startsWith(':')) return
    if (!line.startsWith('data:')) return
    let payload = line.slice(5)
    if (payload.startsWith(' ')) payload = payload.slice(1)
    handlePayload(payload)
  }

  const push = (chunk: string): void => {
    if (done || rawCapped) return
    let input = chunk
    const room = MAX_RAW_BYTES - rawBytes
    if (byteLength(chunk) > room) {
      input = sliceUtf8(chunk, room)
      rawCapped = true
    }
    rawBytes += byteLength(input)
    const combined = buffer + input
    let start = 0
    for (;;) {
      const next = combined.indexOf('\n', start)
      if (next === -1) break
      let line = combined.slice(start, next)
      if (line.endsWith('\r')) line = line.slice(0, -1)
      handleLine(line)
      start = next + 1
    }
    buffer = combined.slice(start)
  }

  const finish = (): ChatResponse => {
    const toolCalls: ChatToolCall[] = []
    for (const [index, call] of [...calls.entries()].sort((a, b) => a[0] - b[0])) {
      if (call.name === '') continue
      toolCalls.push({
        id: call.id === '' ? `call_${index}` : call.id,
        type: 'function',
        function: { name: call.name, arguments: call.args },
      })
    }
    if (errorMessage !== undefined) return { error: { message: errorMessage } }
    if (text === '' && toolCalls.length === 0 && finishReason === undefined) return {}
    return {
      choices: [
        {
          message: {
            role: 'assistant',
            content: text === '' ? null : text,
            ...(toolCalls.length > 0 ? { tool_calls: toolCalls } : {}),
          },
          finish_reason: finishReason,
        },
      ],
    }
  }

  return { push, finish }
}

/**
 * Runs one transport's raw-chunk feed through the parser. Every host adapter is
 * `streamChat((onChunk) => transport(body, onChunk, signal), request.onText)`,
 * so the SSE decoding stays here and the adapters carry only their transport.
 */
export async function streamChat(
  run: (onChunk: (text: string) => void) => Promise<void>,
  onText?: (delta: string) => void,
): Promise<ChatResponse> {
  const parser = createChatStreamParser(onText)
  await run((chunk) => parser.push(chunk))
  return parser.finish()
}
