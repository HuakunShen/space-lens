import type { TauriPorts } from '@space-lens/client'
import { loadTauriPorts } from '../tauri-ports.ts'
import { toChatResponse } from './agent.ts'
import type { ChatPort, ChatRequest, ChatResponse } from './types.ts'

/**
 * Slice C — the live `ChatPort`.
 *
 * The WebView owns no network code for the model: every completion is one
 * `invoke('ai_chat', { messages, tools })` and Rust adds the endpoint, the
 * model and the key (which never comes back to the renderer). The Tauri ports
 * are imported lazily so a browser build never evaluates `@tauri-apps/api`.
 *
 * `ai_get_config` / `ai_set_config` share the same ports and the same
 * write-only key rule: `hasKey` says whether a key is stored, and a `setConfig`
 * that omits `apiKey` keeps whatever is there.
 */

export interface AiConfig {
  baseUrl: string
  model: string
  /** True when a key is stored. The key itself is never handed back. */
  hasKey: boolean
  /** Sends `thinking: { type: 'disabled' }`; much faster on reasoning models (GLM, DeepSeek). */
  disableThinking: boolean
}

export interface AiConfigInput {
  baseUrl: string
  model: string
  /** Omitted keeps the stored key; `null` clears it; a string replaces it. */
  apiKey?: string | null
  /** Omitted keeps the stored value. */
  disableThinking?: boolean
}

/** Config + chat, sharing one lazily loaded set of Tauri ports. */
export interface AiChatPort {
  /** The one non-streaming completion the agent loop consumes. */
  chat: ChatPort
  getConfig(): Promise<AiConfig>
  setConfig(input: AiConfigInput): Promise<AiConfig>
}

export function createAiChatPort(loadPorts: () => Promise<TauriPorts> = loadTauriPorts): AiChatPort {
  let cached: Promise<TauriPorts> | null = null
  const ports = (): Promise<TauriPorts> => (cached ??= loadPorts())

  return {
    async chat(request: ChatRequest): Promise<ChatResponse> {
      const resolved = await ports()
      const body = await resolved.invoke('ai_chat', {
        messages: request.messages,
        ...(request.tools === undefined ? {} : { tools: request.tools }),
      })
      return toChatResponse(body)
    },
    async getConfig(): Promise<AiConfig> {
      const resolved = await ports()
      return readConfig(await resolved.invoke('ai_get_config', {}))
    },
    async setConfig(input: AiConfigInput): Promise<AiConfig> {
      const resolved = await ports()
      const payload: Record<string, unknown> = { baseUrl: input.baseUrl, model: input.model }
      if (input.apiKey !== undefined) payload.apiKey = input.apiKey
      if (input.disableThinking !== undefined) payload.disableThinking = input.disableThinking
      return readConfig(await resolved.invoke('ai_set_config', payload))
    },
  }
}

/** Reads the frozen config reply leniently; a stale host backfills defaults. */
function readConfig(value: unknown): AiConfig {
  if (typeof value !== 'object' || value === null) return { baseUrl: '', model: '', hasKey: false, disableThinking: false }
  const record = value as { baseUrl?: unknown; model?: unknown; hasKey?: unknown; disableThinking?: unknown }
  return {
    baseUrl: typeof record.baseUrl === 'string' ? record.baseUrl : '',
    model: typeof record.model === 'string' ? record.model : '',
    hasKey: record.hasKey === true,
    disableThinking: record.disableThinking === true,
  }
}

/**
 * Browser developer mode (`#ai-dev` in the URL): lets the advisor be driven in
 * a plain browser against `spacelens serve`, where there is no Tauri shell.
 * The key lives in this origin's localStorage, so it is a debugging aid, not a
 * product surface; the shipped browser build never shows the drawer without
 * the hash.
 */
const DEV_STORAGE_KEY = 'spacelens.ai.dev'

export function aiDevActive(): boolean {
  return typeof window !== 'undefined' && window.location.hash.includes('ai-dev')
}

interface DevStored {
  baseUrl: string
  model: string
  apiKey: string
  disableThinking: boolean
}

function readDev(): DevStored {
  try {
    const parsed = JSON.parse(window.localStorage.getItem(DEV_STORAGE_KEY) ?? '{}') as Partial<DevStored>
    return { baseUrl: parsed.baseUrl ?? '', model: parsed.model ?? '', apiKey: parsed.apiKey ?? '', disableThinking: parsed.disableThinking === true }
  } catch {
    return { baseUrl: '', model: '', apiKey: '', disableThinking: false }
  }
}

export function createBrowserDevChatPort(): AiChatPort {
  const view = (stored: DevStored): AiConfig => ({
    baseUrl: stored.baseUrl,
    model: stored.model,
    hasKey: stored.apiKey !== '',
    disableThinking: stored.disableThinking,
  })
  return {
    async chat(request: ChatRequest): Promise<ChatResponse> {
      const stored = readDev()
      if (!stored.model) throw new Error('AI model is not set')
      const base = stored.baseUrl.trim().replace(/\/+$/, '')
      const endpoint = base.endsWith('/chat/completions') ? base : `${base}/chat/completions`
      const response = await fetch(endpoint, {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          ...(stored.apiKey ? { authorization: `Bearer ${stored.apiKey}` } : {}),
        },
        body: JSON.stringify({
          model: stored.model,
          messages: request.messages,
          ...(request.tools?.length ? { tools: request.tools } : {}),
          ...(stored.disableThinking ? { thinking: { type: 'disabled' } } : {}),
        }),
      })
      if (!response.ok) {
        throw new Error(`AI request failed with ${response.status} at ${endpoint}: ${(await response.text()).slice(0, 300)}`)
      }
      return toChatResponse(await response.json())
    },
    async getConfig(): Promise<AiConfig> {
      return view(readDev())
    },
    async setConfig(input: AiConfigInput): Promise<AiConfig> {
      const current = readDev()
      const next: DevStored = {
        baseUrl: input.baseUrl,
        model: input.model,
        apiKey: input.apiKey === undefined ? current.apiKey : (input.apiKey ?? ''),
        disableThinking: input.disableThinking ?? current.disableThinking,
      }
      window.localStorage.setItem(DEV_STORAGE_KEY, JSON.stringify(next))
      return view(next)
    },
  }
}
