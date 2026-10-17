import {
  readConfig,
  toChatResponse,
  type AiChatPort,
  type AiConfig,
  type AiConfigInput,
  type ChatRequest,
  type ChatResponse,
} from '@space-lens/ai-advisor'
import type { TauriPorts } from '@space-lens/client'
import { loadTauriPorts } from '../tauri-ports.ts'

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
