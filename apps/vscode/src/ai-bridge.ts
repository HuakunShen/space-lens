// Extension-host side of the AI advisor (task E of `docs/ai-advisor-plan.md`).
//
// The webview is sandboxed (`default-src 'none'`) and can never reach a model,
// so it sends the conversation through the bridge and this module performs the
// call. Everything the module needs is injected (`state`, `secrets`,
// `fetchImpl`) so it carries no `vscode` import and is unit-testable; the
// server-side transport rules live in `@space-lens/host/ai` and are reused, not
// copied.
//
// Two stores, deliberately split: the non-secret config (base URL, model,
// thinking flag) sits in `state`; the API key sits *only* in `secrets` and is
// never returned — callers learn `hasKey`, nothing more.
import { resolve, sep } from 'node:path'
import {
  CHAT_TIMEOUT_MS,
  MAX_ERROR_BODY_CHARS,
  MAX_RESPONSE_BODY_BYTES,
  buildChatBody,
  chatEndpoint,
  encodeChatBody,
  forwardChatCompletion,
  redact,
  truncate,
} from '@space-lens/host/ai'

/** One key in `globalState`; holds the whole non-secret config object. */
export const AI_CONFIG_KEY = 'spacelens.ai.config'
/** One key in `secrets`; the API key never leaves this store. */
export const AI_SECRET_KEY = 'spacelens.ai.apiKey'

export const DEFAULT_AI_BASE_URL = 'https://api.openai.com/v1'

/**
 * The cap on one streamed response: the total bytes forwarded to `onChunk`
 * before the upstream request is aborted. Larger than the non-streaming
 * `MAX_RESPONSE_BODY_BYTES`, because an SSE body carries framing and every
 * delta, not one compacted JSON document.
 */
export const MAX_STREAM_BODY_BYTES = 4 * 1024 * 1024

/** The slice of `vscode.Memento` this module uses (`get` + `update`). */
export interface AiStateStore {
  get<T>(key: string): T | undefined
  update(key: string, value: unknown): PromiseLike<void>
}

/** The slice of `vscode.SecretStorage` this module uses. */
export interface AiSecretStore {
  get(key: string): PromiseLike<string | undefined>
  store(key: string, value: string): PromiseLike<void>
  delete(key: string): PromiseLike<void>
}

export interface AiBridgeOptions {
  state: AiStateStore
  secrets: AiSecretStore
  /** Test seam; forwarded to `forwardChatCompletion`. Defaults to global fetch. */
  fetchImpl?: typeof fetch | undefined
}

/** What the webview is allowed to see: never the key itself. */
export interface AiConfig {
  baseUrl: string
  model: string
  hasKey: boolean
  disableThinking: boolean
  /** Always false in the VS Code flavor (the key is held by VS Code's secret store). */
  managed: false
}

export interface AiConfigInput {
  /** `undefined` keeps the stored base URL. */
  baseUrl?: string | undefined
  /** `undefined` keeps the stored model. */
  model?: string | undefined
  /**
   * `undefined` keeps the stored key, `null` or `''` clears it, any other
   * string replaces it (trimmed).
   */
  apiKey?: string | null | undefined
  /** `undefined` keeps the stored flag. */
  disableThinking?: boolean | undefined
}

export interface AiChatInput {
  messages: readonly Record<string, unknown>[]
  tools?: readonly Record<string, unknown>[] | undefined
}

export interface AiChatStreamInput extends AiChatInput {
  /** Aborts the upstream request; combined with the 120 s timeout. */
  signal?: AbortSignal | undefined
  /**
   * Called once per decoded, non-empty text slice, in order. Slices are raw
   * SSE bytes (`data: …\n\n` included) — this layer does not parse them.
   */
  onChunk: (text: string) => void
}

export interface AiBridge {
  getConfig(): Promise<AiConfig>
  setConfig(input: AiConfigInput): Promise<AiConfig>
  chat(input: AiChatInput): Promise<unknown>
  chatStream(input: AiChatStreamInput): Promise<void>
  clearKey(): Promise<void>
}

/** The persisted non-secret config; a reader tolerates partial/absent values. */
interface StoredConfig {
  baseUrl: string
  model: string
  disableThinking: boolean
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

/**
 * Reads an error response body under the response cap so a proxy's HTML page is
 * never buffered unbounded; only the first `MAX_ERROR_BODY_CHARS` of the result
 * reach a message (via `truncate`).
 */
async function readErrorText(response: Response): Promise<string> {
  const reader = response.body?.getReader()
  if (reader === undefined) return ''
  const decoder = new TextDecoder()
  let text = ''
  let bytes = 0
  try {
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      if (value === undefined) continue
      bytes += value.length
      text += decoder.decode(value, { stream: true })
      if (bytes > MAX_RESPONSE_BODY_BYTES) break
    }
    text += decoder.decode()
  } catch {
    // A broken error body is still an error; the status and endpoint carry it.
  } finally {
    await reader.cancel().catch(() => undefined)
  }
  return text
}

export function createAiBridge(options: AiBridgeOptions): AiBridge {
  const { state, secrets, fetchImpl } = options

  function readConfig(): StoredConfig {
    const raw = state.get<Partial<StoredConfig>>(AI_CONFIG_KEY)
    const baseUrl =
      typeof raw?.baseUrl === 'string' && raw.baseUrl.trim() !== '' ? raw.baseUrl : DEFAULT_AI_BASE_URL
    const model = typeof raw?.model === 'string' ? raw.model : ''
    const disableThinking = raw?.disableThinking === true
    return { baseUrl, model, disableThinking }
  }

  async function readKey(): Promise<string | null> {
    const key = await secrets.get(AI_SECRET_KEY)
    return key === undefined || key === '' ? null : key
  }

  async function getConfig(): Promise<AiConfig> {
    const stored = readConfig()
    const key = await secrets.get(AI_SECRET_KEY)
    return {
      baseUrl: stored.baseUrl,
      model: stored.model,
      hasKey: typeof key === 'string' && key !== '',
      disableThinking: stored.disableThinking,
      managed: false,
    }
  }

  async function setConfig(input: AiConfigInput): Promise<AiConfig> {
    const current = readConfig()
    const baseUrl = input.baseUrl === undefined ? current.baseUrl : input.baseUrl.trim()
    // The one place a base URL becomes a request target; validating here means a
    // bad value is refused at configuration time, not at chat time.
    try {
      chatEndpoint(baseUrl)
    } catch (error) {
      throw new Error(`AI base URL is invalid: ${messageOf(error)}`)
    }
    const model = input.model === undefined ? current.model : input.model
    const disableThinking = input.disableThinking === undefined ? current.disableThinking : input.disableThinking === true
    await state.update(AI_CONFIG_KEY, { baseUrl, model, disableThinking })

    if (input.apiKey !== undefined) {
      if (input.apiKey === null || input.apiKey.trim() === '') {
        await secrets.delete(AI_SECRET_KEY)
      } else {
        await secrets.store(AI_SECRET_KEY, input.apiKey.trim())
      }
    }
    return getConfig()
  }

  async function chat(input: AiChatInput): Promise<unknown> {
    const stored = readConfig()
    if (stored.model.trim() === '') throw new Error('AI model is not set')
    const apiKey = await readKey()
    try {
      return await forwardChatCompletion({
        endpoint: chatEndpoint(stored.baseUrl),
        model: stored.model,
        apiKey,
        disableThinking: stored.disableThinking,
        messages: input.messages,
        tools: input.tools,
        fetchImpl,
      })
    } catch (error) {
      // `forwardChatCompletion` already redacts, but any message built here must
      // be redacted too: the key must never reach the webview.
      throw new Error(redact(messageOf(error), apiKey))
    }
  }

  async function chatStream(input: AiChatStreamInput): Promise<void> {
    const stored = readConfig()
    const apiKey = await readKey()
    try {
      if (stored.model.trim() === '') throw new Error('AI model is not set')
      const endpoint = chatEndpoint(stored.baseUrl)

      // The 1 MiB request cap is enforced (and refused) before the key is
      // attached; `stream: true` is what makes the response an SSE body.
      const body = encodeChatBody({
        ...buildChatBody({
          model: stored.model,
          messages: input.messages,
          tools: input.tools,
          disableThinking: stored.disableThinking,
        }),
        stream: true,
      })

      const headers: Record<string, string> = { 'content-type': 'application/json' }
      if (apiKey !== null) headers.authorization = `Bearer ${apiKey}`

      // The 120 s budget is a total: the caller's signal can end it sooner, and
      // the timeout can end a stalled body early. `AbortSignal.any` is what
      // makes cancellation abort the upstream request.
      const timeout = AbortSignal.timeout(CHAT_TIMEOUT_MS)
      const signal = input.signal === undefined ? timeout : AbortSignal.any([timeout, input.signal])

      let response: Response
      try {
        response = await (fetchImpl ?? fetch)(endpoint, {
          method: 'POST',
          headers,
          body,
          redirect: 'error',
          signal,
        })
      } catch (error) {
        throw new Error(`AI request failed: ${messageOf(error)}`)
      }

      if (!response.ok) {
        // Same shape as the non-streaming path: status, endpoint, and 300
        // characters of the (capped) body, with the key redacted at the end.
        const text = await readErrorText(response)
        throw new Error(
          `AI request failed with ${response.status} at ${endpoint}: ${truncate(text, MAX_ERROR_BODY_CHARS)}`,
        )
      }

      const reader = response.body?.getReader()
      if (reader === undefined) return
      const decoder = new TextDecoder()
      let total = 0
      try {
        for (;;) {
          const { done, value } = await reader.read()
          if (done) break
          if (value === undefined) continue
          total += value.length
          if (total > MAX_STREAM_BODY_BYTES) {
            throw new Error(`AI streamed response is too large (over ${MAX_STREAM_BODY_BYTES} bytes)`)
          }
          const text = decoder.decode(value, { stream: true })
          if (text !== '') input.onChunk(text)
        }
        // Flush any bytes a multibyte character left buffered at the boundary.
        const tail = decoder.decode()
        if (tail !== '') input.onChunk(tail)
      } finally {
        // No-op after a completed read; on error/abort/cap this releases the
        // connection instead of waiting for the rest of the body.
        await reader.cancel().catch(() => undefined)
      }
    } catch (error) {
      // Every message this function throws passes through here: the key must
      // never reach the webview.
      throw new Error(redact(messageOf(error), apiKey))
    }
  }

  async function clearKey(): Promise<void> {
    await secrets.delete(AI_SECRET_KEY)
  }

  return { getConfig, setConfig, chat, chatStream, clearKey }
}

/**
 * Lexical containment check used by `reveal`: is `candidate` the root itself or
 * a path inside it? Built on `path.resolve` so `..` segments are normalized
 * away, and compared with a trailing separator so a sibling that merely shares
 * a prefix (`/a/bc` vs `/a/b`) is not mistaken for a child.
 */
export function isPathWithin(root: string, candidate: string): boolean {
  const resolvedRoot = resolve(root)
  const resolvedCandidate = resolve(candidate)
  if (resolvedCandidate === resolvedRoot) return true
  const rootWithSeparator = resolvedRoot.endsWith(sep) ? resolvedRoot : `${resolvedRoot}${sep}`
  return resolvedCandidate.startsWith(rootWithSeparator)
}
