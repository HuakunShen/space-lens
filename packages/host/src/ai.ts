import { ProblemError, Problems } from './problems.ts'

/**
 * Server-side AI forwarding (task H of `docs/ai-advisor-plan.md`).
 *
 * A browser or DSH page served by this host has a CSP of `connect-src 'self'`
 * and therefore cannot reach a model endpoint itself. The host forwards the
 * one non-streaming chat completion the advisor loop needs, adding the
 * operator-configured model and — when present — the API key. The endpoint,
 * model and key are never client-settable: the only input is the conversation
 * and tool schemas.
 *
 * The rules mirror `apps/desktop/src-tauri/src/ai.rs` (`chat_endpoint`,
 * `chat_body`, the size caps and the redaction): https unless loopback, no
 * userinfo, one appended `/chat/completions`, and error messages that never
 * carry the key.
 */

/** Long enough for a slow tool-calling completion, short enough to fail before a user gives up. */
export const CHAT_TIMEOUT_MS = 120_000
/** `docs/ai-advisor-plan.md` item 5: 1 MiB of request, 2 MiB of response. */
export const MAX_REQUEST_BODY_BYTES = 1024 * 1024
export const MAX_RESPONSE_BODY_BYTES = 2 * 1024 * 1024
/** A streamed reply is forwarded, not buffered, but still has a hard ceiling. */
export const MAX_STREAM_BODY_BYTES = 4 * 1024 * 1024
/** The response tail kept in an error message: enough to see "invalid api key", never a proxy's HTML page. */
export const MAX_ERROR_BODY_CHARS = 300
export const CHAT_COMPLETIONS_PATH = '/chat/completions'

/**
 * Where one chat completion goes. Pure and total — the only place a base URL
 * becomes a request target, so the transport rule cannot be bypassed by a
 * later caller. Also used by `resolveServeConfig` to validate the operator's
 * base URL at startup (wrapped into a `ConfigError` there).
 *
 * `https` is required, except for loopback (`localhost`, `127.0.0.1`, `::1`)
 * where plaintext never leaves the machine — that is what makes a local
 * llama.cpp/ollama endpoint usable without weakening remote calls. Userinfo is
 * rejected so credentials cannot hide inside the URL, and the path is appended
 * after trimming trailing slashes so `…/v1/` and `…/v1` produce the same
 * endpoint.
 */
export function chatEndpoint(baseUrl: string): string {
  const base = baseUrl.trim().replace(/\/+$/, '')
  if (base === '') throw new Error('AI base URL is not set')
  let parsed: URL
  try {
    parsed = new URL(base)
  } catch {
    throw new Error('AI base URL is not a valid URL')
  }
  if (parsed.username !== '' || parsed.password !== '') {
    throw new Error('AI base URL must not contain credentials')
  }
  const scheme = parsed.protocol.replace(/:$/, '')
  if (scheme === 'https') {
    // remote https
  } else if (scheme === 'http' && isLoopbackHost(parsed.hostname)) {
    // plaintext is accepted only where it never leaves the machine
  } else if (scheme === 'http') {
    throw new Error('AI base URL must use https unless the host is localhost')
  } else {
    throw new Error(`AI base URL scheme is not supported: ${scheme}`)
  }
  // Pasting the full endpoint is a common slip; appending again would 404.
  if (base.endsWith(CHAT_COMPLETIONS_PATH)) return base
  return `${base}${CHAT_COMPLETIONS_PATH}`
}

function isLoopbackHost(hostname: string): boolean {
  return hostname === 'localhost' || hostname === '127.0.0.1' || hostname === '::1' || hostname === '[::1]'
}

export interface ChatBodyInput {
  model: string
  messages: readonly Record<string, unknown>[]
  tools?: readonly Record<string, unknown>[] | undefined
  disableThinking: boolean
}

/**
 * `temperature` is deliberately absent: OpenAI-compatible servers disagree on
 * its default and the agent loop wants the model's own default. `tools` is
 * omitted rather than sent as `[]` — several compatible servers reject an
 * empty array outright.
 */
export function buildChatBody(input: ChatBodyInput): Record<string, unknown> {
  const body: Record<string, unknown> = { model: input.model, messages: input.messages }
  if (input.disableThinking) body.thinking = { type: 'disabled' }
  if (input.tools !== undefined && input.tools.length > 0) body.tools = input.tools
  return body
}

/** The one cap predicate, shared by both directions so the two limits cannot drift. */
function ensureWithinCap(label: string, length: number, cap: number): void {
  if (length > cap) {
    throw new Error(`${label} is too large (${length} bytes; the limit is ${cap} bytes)`)
  }
}

const encoder = new TextEncoder()

/**
 * Serializes one request body and enforces the 1 MiB cap before anything is
 * sent. The returned bytes are exactly what goes on the wire, so the checked
 * size and the sent size cannot disagree.
 */
export function encodeChatBody(body: unknown): Uint8Array<ArrayBuffer> {
  const bytes = encoder.encode(JSON.stringify(body))
  // Only byte counts reach this message, so there is nothing to redact.
  ensureWithinCap('AI request body', bytes.length, MAX_REQUEST_BODY_BYTES)
  return bytes
}

/**
 * Appends one chunk under a hard cap. A chunk that would cross the cap is
 * refused *before* it is buffered, so an oversized body costs one chunk of
 * memory instead of the whole body.
 */
export function appendCapped(buffer: Uint8Array, chunk: Uint8Array, cap: number): Uint8Array {
  ensureWithinCap('AI response body', buffer.length + chunk.length, cap)
  const merged = new Uint8Array(buffer.length + chunk.length)
  merged.set(buffer, 0)
  merged.set(chunk, buffer.length)
  return merged
}

/**
 * Reads the body chunk by chunk under the cap. Returning early drops (cancels)
 * the response — which aborts the connection — so the body is never buffered
 * unbounded.
 */
export async function readCappedBody(response: Response, cap: number): Promise<Uint8Array> {
  const reader = response.body?.getReader()
  if (reader === undefined) return new Uint8Array()
  let buffer: Uint8Array = new Uint8Array()
  try {
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      if (value !== undefined) buffer = appendCapped(buffer, value, cap)
    }
  } catch (error) {
    await reader.cancel().catch(() => undefined)
    throw error
  }
  return buffer
}

/** Every error message crosses this: an API key must never appear in a string that reaches a client or a log. */
export function redact(message: string, key: string | null | undefined): string {
  if (key === null || key === undefined || key === '') return message
  return message.replaceAll(key, '[redacted]')
}

export function truncate(text: string, maxChars: number): string {
  const chars = Array.from(text)
  if (chars.length <= maxChars) return text
  return `${chars.slice(0, maxChars).join('')}…`
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function isTimeout(error: unknown): boolean {
  return error instanceof Error && (error.name === 'TimeoutError' || error.name === 'AbortError')
}

export interface ForwardChatInput {
  /** Validated chat-completions endpoint (`chatEndpoint` output). */
  endpoint: string
  model: string
  /** The key, kept here and never echoed. `null` sends no Authorization header. */
  apiKey: string | null
  disableThinking: boolean
  messages: readonly Record<string, unknown>[]
  tools?: readonly Record<string, unknown>[] | undefined
  /** Test seam; defaults to global fetch. */
  fetchImpl?: typeof fetch | undefined
}

/**
 * Forwards one non-streaming chat completion and returns the parsed JSON body.
 * The client sends the whole conversation (and tool schemas); the host adds the
 * model, the key, and the endpoint.
 */
export async function forwardChatCompletion(input: ForwardChatInput): Promise<unknown> {
  const doFetch = input.fetchImpl ?? fetch
  // Checked before the key is attached: the 1 MiB cap refuses an oversized
  // transcript without ever building a request.
  let body: Uint8Array<ArrayBuffer>
  try {
    body = encodeChatBody(
      buildChatBody({
        model: input.model,
        messages: input.messages,
        tools: input.tools,
        disableThinking: input.disableThinking,
      }),
    )
  } catch (error) {
    throw Problems.invalid(errorMessage(error))
  }

  const headers: Record<string, string> = { 'content-type': 'application/json' }
  if (input.apiKey !== null) headers.authorization = `Bearer ${input.apiKey}`

  let response: Response
  try {
    response = await doFetch(input.endpoint, {
      method: 'POST',
      headers,
      body,
      // The closed set: no redirects (a redirect could smuggle a foreign target).
      redirect: 'error',
      signal: AbortSignal.timeout(CHAT_TIMEOUT_MS),
    })
  } catch (error) {
    if (isTimeout(error)) throw Problems.timeout('AI request timed out after 120s')
    throw Problems.unavailable(redact(`AI request failed: ${errorMessage(error)}`, input.apiKey))
  }

  let text: string
  try {
    text = new TextDecoder().decode(await readCappedBody(response, MAX_RESPONSE_BODY_BYTES))
  } catch (error) {
    if (error instanceof ProblemError) throw error
    throw Problems.unavailable(redact(errorMessage(error), input.apiKey))
  }

  if (!response.ok) {
    throw Problems.unavailable(
      redact(
        `AI request failed with ${response.status} at ${input.endpoint}: ${truncate(text, MAX_ERROR_BODY_CHARS)}`,
        input.apiKey,
      ),
    )
  }

  try {
    return JSON.parse(text) as unknown
  } catch (error) {
    throw Problems.unavailable(
      redact(
        `AI response was not JSON (${errorMessage(error)}): ${truncate(text, MAX_ERROR_BODY_CHARS)}`,
        input.apiKey,
      ),
    )
  }
}

/**
 * Forwards one streaming chat completion. The request carries `stream: true`
 * and the upstream response body is passed through as raw text chunks — never
 * parsed here. A shared parser in `@space-lens/ai-advisor` assembles the deltas.
 *
 * The 1 MiB request cap is the same as the non-streaming call and is checked
 * before the key is attached. A non-2xx upstream reads the (capped) body and
 * throws the same redacted `Problems.unavailable`, so the route can still answer
 * a problem JSON before any byte is sent. The forwarded bytes are capped at
 * 4 MiB: the crossing chunk is refused, the upstream reader is cancelled, and
 * the returned stream errors. Cancelling the returned stream (or aborting the
 * caller's signal) cancels the upstream request.
 */
export async function forwardChatStream(
  input: ForwardChatInput & { signal?: AbortSignal | undefined },
): Promise<ReadableStream<Uint8Array>> {
  const doFetch = input.fetchImpl ?? fetch
  // Checked before the key is attached, exactly like the non-streaming call.
  let body: Uint8Array<ArrayBuffer>
  try {
    body = encodeChatBody({
      ...buildChatBody({
        model: input.model,
        messages: input.messages,
        tools: input.tools,
        disableThinking: input.disableThinking,
      }),
      stream: true,
    })
  } catch (error) {
    throw Problems.invalid(errorMessage(error))
  }

  const headers: Record<string, string> = { 'content-type': 'application/json' }
  if (input.apiKey !== null) headers.authorization = `Bearer ${input.apiKey}`

  const timeout = AbortSignal.timeout(CHAT_TIMEOUT_MS)
  const signal = input.signal === undefined ? timeout : AbortSignal.any([timeout, input.signal])

  let response: Response
  try {
    response = await doFetch(input.endpoint, { method: 'POST', headers, body, redirect: 'error', signal })
  } catch (error) {
    if (isTimeout(error)) throw Problems.timeout('AI request timed out after 120s')
    throw Problems.unavailable(redact(`AI request failed: ${errorMessage(error)}`, input.apiKey))
  }

  if (!response.ok) {
    let text: string
    try {
      text = new TextDecoder().decode(await readCappedBody(response, MAX_RESPONSE_BODY_BYTES))
    } catch (error) {
      if (error instanceof ProblemError) throw error
      throw Problems.unavailable(redact(errorMessage(error), input.apiKey))
    }
    throw Problems.unavailable(
      redact(
        `AI request failed with ${response.status} at ${input.endpoint}: ${truncate(text, MAX_ERROR_BODY_CHARS)}`,
        input.apiKey,
      ),
    )
  }

  const upstream = response.body?.getReader()
  if (upstream === undefined) {
    return new ReadableStream<Uint8Array>({ start: (controller) => controller.close() })
  }

  let forwarded = 0
  return new ReadableStream<Uint8Array>({
    async pull(controller) {
      let read: Awaited<ReturnType<typeof upstream.read>>
      try {
        read = await upstream.read()
      } catch (error) {
        try {
          controller.error(error)
        } catch {
          // the consumer already closed or cancelled the stream
        }
        return
      }
      if (read.done) {
        try {
          controller.close()
        } catch {
          // already closed
        }
        return
      }
      const chunk = read.value
      if (chunk === undefined) return
      if (forwarded + chunk.length > MAX_STREAM_BODY_BYTES) {
        // Refuse the crossing chunk, drop the upstream, and fail the stream at
        // the same place the non-streaming path fails its cap.
        await upstream.cancel().catch(() => undefined)
        try {
          controller.error(
            Problems.unavailable(
              redact(
                `AI response body is too large (${forwarded + chunk.length} bytes; the limit is ${MAX_STREAM_BODY_BYTES} bytes)`,
                input.apiKey,
              ),
            ),
          )
        } catch {
          // already closed
        }
        return
      }
      forwarded += chunk.length
      try {
        controller.enqueue(chunk)
      } catch {
        // already closed
      }
    },
    async cancel(reason) {
      await upstream.cancel(reason).catch(() => undefined)
    },
  })
}
