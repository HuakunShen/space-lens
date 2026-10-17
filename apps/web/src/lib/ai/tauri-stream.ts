/**
 * Slice S2 — the streaming transport for the Tauri desktop shell.
 *
 * `ai_chat_stream` forwards the raw text chunks of an upstream
 * OpenAI-compatible `stream: true` body over a Tauri **Channel**; the SSE text
 * is assembled by `@space-lens/ai-advisor`'s stream parser, never here. The
 * `@tauri-apps/api` import stays lazy so a browser build never evaluates Tauri
 * code — the same rule `tauri-ports.ts` and `chat-port.ts` follow.
 */

/** The subset of a chat request a streamed completion needs. */
export interface TauriStreamChatBody {
  messages: unknown[]
  tools?: unknown[]
}

export type TauriStreamChunkHandler = (text: string) => void

/**
 * Runs one streamed completion and resolves when the upstream body ends.
 *
 * `onChunk` receives the raw body text as it arrives. Rust carries a UTF-8
 * character that was split across two upstream TCP chunks into the next chunk,
 * so a chunk never ends mid-character. Aborting `signal` cancels the run
 * upstream and makes the promise reject with the error string.
 */
export async function streamTauriChat(
  body: TauriStreamChatBody,
  onChunk: TauriStreamChunkHandler,
  signal?: AbortSignal,
): Promise<void> {
  const { invoke, Channel } = await import('@tauri-apps/api/core')
  const runId = crypto.randomUUID()
  const channel = new Channel<string>()
  channel.onmessage = onChunk

  // Like `chat-port.ts`, a transport failure must surface as an `Error`, not a
  // bare string: Rust returns `Err(String)` and Tauri forwards it verbatim.
  const fail = (error: unknown): never => {
    throw error instanceof Error ? error : new Error(String(error))
  }

  if (signal?.aborted) fail('cancelled')

  const cancel = (): void => {
    void invoke('ai_chat_cancel', { runId }).catch(() => {})
  }
  signal?.addEventListener('abort', cancel, { once: true })

  try {
    await invoke('ai_chat_stream', {
      messages: body.messages,
      ...(body.tools === undefined ? {} : { tools: body.tools }),
      runId,
      onChunk: channel,
    })
  } catch (error) {
    fail(error)
  } finally {
    signal?.removeEventListener('abort', cancel)
  }
}
