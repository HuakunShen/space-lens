import { z } from 'zod'

/**
 * Read-only view of the host's operator-configured AI advisor. The host owns
 * the base URL, model and key; there is deliberately no client-facing route
 * that changes any of them. `managed` is a literal `true` so a client can tell
 * this apart from the BYOK desktop config shape (`AiConfig`), and in
 * particular can see that the endpoint is not client-settable.
 */
export const AiConfigViewSchema = z.strictObject({
  baseUrl: z.string(),
  model: z.string(),
  /** True when the host holds an API key. The key itself is never returned. */
  hasKey: z.boolean(),
  disableThinking: z.boolean(),
  managed: z.literal(true),
})
export type AiConfigView = z.infer<typeof AiConfigViewSchema>

/** One OpenAI-compatible message. The host forwards it verbatim. */
const AiMessageSchema = z.record(z.string(), z.unknown())

export const AiChatRequestSchema = z.strictObject({
  messages: z.array(AiMessageSchema).min(1).max(2000),
  tools: z.array(z.record(z.string(), z.unknown())).max(64).optional(),
  /**
   * When `true` the host forwards the upstream body as `text/event-stream` raw
   * chunks instead of one parsed JSON body. Optional so every existing caller
   * (and host) keeps working unchanged.
   */
  stream: z.boolean().optional(),
})
export type AiChatRequest = z.infer<typeof AiChatRequestSchema>
