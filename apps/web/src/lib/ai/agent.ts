import type { CollectorEntry } from '@space-lens/contract'
import { SYSTEM_PROMPT } from './prompts.ts'
import {
  AGENT_LIMITS,
  AI_TOOLS,
  buildInitialContext,
  budgetStopNotice,
  byteLength,
  createToolRuntime,
  executeTool,
  rememberNodes,
  truncateUtf8,
  type InitialContext,
} from './tools.ts'
import type { ChatMessage, ChatPort, ChatResponse, ChatToolCall, LensPort } from './types.ts'

/**
 * Slice A agent loop for whole-disk scans.
 *
 * Context is a budget, not an archive: before every round, tool results older
 * than the last 2 rounds are replaced by a one-line summary (their ids stay in
 * the seen set), and the whole context is held under `maxContextTokens`. The
 * conversation itself carries the initial window and the model navigates by
 * `nodeId` from there.
 */

export type AgentEndReason = 'final' | 'rounds' | 'budget' | 'error'

export type AgentEvent =
  | { type: 'assistant'; round: number; text: string | null; toolCalls: string[] }
  | { type: 'tool'; round: number; name: string; content: string }
  | { type: 'staged'; entries: CollectorEntry[] }
  | { type: 'end'; reason: AgentEndReason }

export interface AgentStats {
  rounds: number
  toolCalls: number
  /** Raw bytes of tool output sent this run. */
  bytesSent: number
  /** Estimated tokens across every request sent (tools included). */
  approxTokensSent: number
}

export interface RunAgentOptions {
  chat: ChatPort
  lens: LensPort
  userMessage: string
  /** The user's open folder; omitted means the scan root. */
  focusNodeId?: string
  /** Earlier turns, oldest first, without the system prompt. */
  history?: ChatMessage[]
  onEvent?: (event: AgentEvent) => void
  signal?: AbortSignal
}

export interface AgentResult {
  text: string
  messages: ChatMessage[]
  stats: AgentStats
  staged: CollectorEntry[]
  endReason: AgentEndReason
}

/** Assistant prose is not otherwise budgeted; cap it so one reply cannot blow the window. */
const ASSISTANT_MAX_BYTES = 2 * 1024
const TOOL_SCHEMA_BYTES = byteLength(JSON.stringify(AI_TOOLS))

export function estimateContextTokens(messages: readonly ChatMessage[]): number {
  let bytes = TOOL_SCHEMA_BYTES
  for (const message of messages) {
    if (message.content !== null) bytes += byteLength(message.content)
    if (message.name !== undefined) bytes += byteLength(message.name)
    for (const call of message.tool_calls ?? []) {
      bytes += byteLength(call.id) + byteLength(call.function.name) + byteLength(call.function.arguments) + 16
    }
  }
  return Math.ceil(bytes / AGENT_LIMITS.bytesPerToken)
}

interface ToolMeta {
  message: ChatMessage
  round: number
  summary: string
  compacted: boolean
}

export async function runAgent(options: RunAgentOptions): Promise<AgentResult> {
  const runtime = createToolRuntime()

  let context: InitialContext
  try {
    context = await buildInitialContext(options.lens, options.focusNodeId)
  } catch (error) {
    return {
      text: `Could not read the scan view: ${describe(error)}`,
      messages: [],
      stats: { rounds: 0, toolCalls: 0, bytesSent: 0, approxTokensSent: 0 },
      staged: [],
      endReason: 'error',
    }
  }
  rememberNodes(runtime, context.nodes)

  const history = options.history ?? []
  // The initial window rides in this run's first user message.
  const firstUser: ChatMessage = { role: 'user', content: `${context.text}\n\n${options.userMessage}` }
  const messages: ChatMessage[] = [{ role: 'system', content: SYSTEM_PROMPT }, ...history, firstUser]
  const initial = { message: firstUser, text: context.text, userMessage: options.userMessage }

  const meta: ToolMeta[] = []
  let rounds = 0
  let endReason: AgentEndReason = 'rounds'
  let text = ''
  let approxTokensSent = 0

  while (rounds < AGENT_LIMITS.maxRounds) {
    if (options.signal?.aborted) {
      endReason = 'error'
      text = 'The run was cancelled.'
      break
    }
    rounds += 1
    compact(messages, meta, rounds - 1, initial)

    const sent = estimateContextTokens(messages)
    if (sent > AGENT_LIMITS.maxContextTokens) {
      endReason = 'budget'
      text = `Stopped early: context budget of ${AGENT_LIMITS.maxContextTokens} tokens reached.`
      break
    }
    approxTokensSent += sent

    let response: ChatResponse
    try {
      response = await options.chat({ messages, tools: [...AI_TOOLS] })
    } catch (error) {
      endReason = 'error'
      text = `The model request failed: ${describe(error)}`
      break
    }

    const assistant = assistantMessage(response)
    if (assistant === null) {
      endReason = 'error'
      text = 'The model returned no message.'
      break
    }
    messages.push(assistant)

    const calls = assistant.tool_calls ?? []
    options.onEvent?.({
      type: 'assistant',
      round: rounds,
      text: assistant.content,
      toolCalls: calls.map((call) => call.function.name),
    })

    if (calls.length === 0) {
      text = assistant.content ?? ''
      endReason = 'final'
      break
    }

    for (const call of calls) {
      // Once a guard trips, answer the remaining calls so the transcript stays
      // consistent, then stop: the model gets no further round.
      const outcome =
        runtime.stopReason === null
          ? await executeTool(runtime, options.lens, call.function.name, call.function.arguments)
          : { content: budgetStopNotice(runtime.stopReason), summary: `budget: ${runtime.stopReason}` }
      const message: ChatMessage = {
        role: 'tool',
        tool_call_id: call.id,
        name: call.function.name,
        content: outcome.content,
      }
      messages.push(message)
      meta.push({ message, round: rounds, summary: outcome.summary, compacted: false })
      options.onEvent?.({ type: 'tool', round: rounds, name: call.function.name, content: outcome.content })
    }

    if (runtime.stopReason !== null) {
      endReason = 'budget'
      text = `Stopped early: ${runtime.stopReason}.`
      break
    }
  }

  if (endReason === 'rounds') {
    text = `Stopped after ${AGENT_LIMITS.maxRounds} rounds without a final answer.`
  }
  if (runtime.staged.length > 0) {
    options.onEvent?.({ type: 'staged', entries: [...runtime.staged] })
  }
  options.onEvent?.({ type: 'end', reason: endReason })

  return {
    text,
    messages,
    stats: {
      rounds,
      toolCalls: runtime.toolCalls,
      bytesSent: runtime.toolOutputBytes,
      approxTokensSent,
    },
    staged: [...runtime.staged],
    endReason,
  }
}

/**
 * Bounds the context before a request:
 * 1. results older than the last 2 rounds become one-line summaries,
 * 2. if still over budget, the remaining results are summarised oldest first,
 * 3. oversized assistant prose is trimmed, then the initial window shrinks.
 * Ids stay in the seen set throughout: only the transcript text changes.
 */
function compact(
  messages: ChatMessage[],
  meta: ToolMeta[],
  completedRounds: number,
  initial: { message: ChatMessage; text: string; userMessage: string },
): void {
  const under = (): boolean => estimateContextTokens(messages) <= AGENT_LIMITS.maxContextTokens
  for (const item of meta) {
    if (!item.compacted && item.round <= completedRounds - 2) summarize(item)
  }
  if (under()) return

  for (const item of meta) {
    summarize(item)
    if (under()) return
  }
  for (const message of messages) {
    if (message.role !== 'assistant' || message.content === null) continue
    if (byteLength(message.content) > ASSISTANT_MAX_BYTES) {
      message.content = truncateUtf8(message.content, ASSISTANT_MAX_BYTES)
    }
    if (under()) return
  }
  initial.message.content = `${truncateUtf8(initial.text, 512)}\n\n${initial.userMessage}`
}

function summarize(item: ToolMeta): void {
  if (item.compacted) return
  item.message.content = item.summary
  item.compacted = true
}

function assistantMessage(response: ChatResponse): ChatMessage | null {
  const choice = response.choices?.[0]
  const message = choice?.message
  if (!isChoiceMessage(message)) return null
  const toolCalls = Array.isArray(message.tool_calls) ? message.tool_calls.filter(isToolCall) : []
  const content = typeof message.content === 'string' ? message.content : null
  return {
    role: 'assistant',
    content,
    ...(toolCalls.length > 0 ? { tool_calls: toolCalls } : {}),
  }
}

function isChoiceMessage(value: unknown): value is ChatMessage {
  return typeof value === 'object' && value !== null
}

function isToolCall(value: unknown): value is ChatToolCall {
  if (typeof value !== 'object' || value === null) return false
  const call = value as ChatToolCall
  return (
    typeof call.id === 'string' &&
    typeof call.function === 'object' &&
    call.function !== null &&
    typeof call.function.name === 'string' &&
    typeof call.function.arguments === 'string'
  )
}

/**
 * Reads one `/chat/completions` body leniently. The live port uses this so a
 * host answering an unexpected shape degrades to "no message" instead of
 * throwing inside the loop.
 */
export function toChatResponse(body: unknown): ChatResponse {
  if (typeof body !== 'object' || body === null) return {}
  const record = body as { choices?: unknown }
  const choices = Array.isArray(record.choices) ? (record.choices as ChatResponse['choices']) : undefined
  return choices === undefined ? {} : { choices }
}

function describe(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error)
  return message.length > 300 ? `${message.slice(0, 300)}…` : message
}
