import type { AiChatRequest } from '@space-lens/contract'
import {
  readConfig,
  toChatResponse,
  type AiChatPort,
  type AiConfig,
  type AiConfigInput,
  type ChatRequest,
  type ChatResponse,
} from '@space-lens/ai-advisor'
import type { TauriPorts, WorkbenchService } from '@space-lens/client'
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
 * The HTTP host's advisor (`AiConfigView` + `/api/v1/ai/chat`). The operator
 * owns the base URL, model and key, so the config here is read-only: it comes
 * back with `managed: true` and `setConfig` refuses. The browser build and the
 * DSH plugin talk to `spacelens serve` this way; the endpoint and key never
 * reach the page.
 */
export function createServiceAiChatPort(service: Pick<WorkbenchService, 'aiConfig' | 'aiChat'>): AiChatPort {
  return {
    async chat(request: ChatRequest): Promise<ChatResponse> {
      const aiChat = service.aiChat
      if (aiChat === undefined) throw new Error('The AI advisor is not available over this host')
      // The advisor's closed `ChatMessage` shape and the contract's open
      // record differ only structurally; the host forwards the JSON verbatim.
      const body = {
        messages: request.messages,
        ...(request.tools === undefined ? {} : { tools: request.tools }),
      } as unknown as AiChatRequest
      return toChatResponse(await aiChat(body))
    },
    async getConfig(): Promise<AiConfig> {
      const aiConfig = service.aiConfig
      if (aiConfig === undefined) throw new Error('The AI advisor is not available over this host')
      const view = await aiConfig()
      return { ...readConfig(view), managed: true }
    },
    async setConfig(): Promise<AiConfig> {
      throw new Error('The AI model and key are configured on the host')
    },
  }
}
