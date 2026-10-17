import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { AiConfigView } from '@space-lens/contract'
import type { TauriPorts, WorkbenchService } from '@space-lens/client'
import type { ChatTool } from '@space-lens/ai-advisor'
import { createAiChatPort, createServiceAiChatPort } from '../src/lib/ai/chat-port.ts'

function fakePorts(reply: unknown) {
  const calls: { cmd: string; args: unknown }[] = []
  const ports = {
    invoke: async (cmd: string, args: unknown) => {
      calls.push({ cmd, args })
      return reply
    },
  } as unknown as TauriPorts
  return { ports, calls }
}

test('setConfig forwards disableThinking only when given and reads it back', async () => {
  const { ports, calls } = fakePorts({ baseUrl: 'https://x/v1', model: 'm', hasKey: true, disableThinking: true })
  const port = createAiChatPort(async () => ports)
  const next = await port.setConfig({ baseUrl: 'https://x/v1', model: 'm', disableThinking: true })
  assert.equal(next.disableThinking, true)
  assert.deepEqual(calls[0], {
    cmd: 'ai_set_config',
    args: { baseUrl: 'https://x/v1', model: 'm', disableThinking: true },
  })
  await port.setConfig({ baseUrl: 'https://x/v1', model: 'm' })
  assert.ok(!('disableThinking' in (calls[1]!.args as object)))
})

test('a stale host reply without the field reads as thinking enabled', async () => {
  const { ports } = fakePorts({ baseUrl: 'u', model: 'm', hasKey: false })
  const config = await createAiChatPort(async () => ports).getConfig()
  assert.equal(config.disableThinking, false)
})

const hostView = {
  baseUrl: 'https://host.example/v1',
  model: 'host-model',
  hasKey: true,
  disableThinking: true,
  managed: true,
} satisfies AiConfigView

const tool: ChatTool = {
  type: 'function',
  function: {
    name: 'get_scan_summary',
    description: 'd',
    parameters: { type: 'object', properties: {}, additionalProperties: false },
  },
}

function fakeService(aiChat: (body: unknown) => Promise<unknown>) {
  return {
    aiConfig: async () => hostView,
    aiChat,
  } as Pick<WorkbenchService, 'aiConfig' | 'aiChat'>
}

test('the service port maps the host config and always reports it as managed', async () => {
  const port = createServiceAiChatPort(fakeService(async () => ({})))
  const config = await port.getConfig()
  assert.deepEqual(config, {
    baseUrl: 'https://host.example/v1',
    model: 'host-model',
    hasKey: true,
    disableThinking: true,
    managed: true,
  })
})

test('the service port reads a lenient host config with defaults', async () => {
  const port = createServiceAiChatPort({
    aiConfig: async () => ({ model: 'm' }) as unknown as AiConfigView,
    aiChat: async () => ({}),
  } as Pick<WorkbenchService, 'aiConfig' | 'aiChat'>)
  const config = await port.getConfig()
  assert.deepEqual(config, { baseUrl: '', model: 'm', hasKey: false, disableThinking: false, managed: true })
})

test('the service port refuses to write the host-owned config', async () => {
  const port = createServiceAiChatPort(fakeService(async () => ({})))
  await assert.rejects(
    () => port.setConfig({ baseUrl: 'https://elsewhere/v1', model: 'other' }),
    /configured on the host/,
  )
})

test('the service port forwards messages and tools and normalizes the reply', async () => {
  const seen: unknown[] = []
  const port = createServiceAiChatPort(
    fakeService(async (body) => {
      seen.push(body)
      return { choices: [{ message: { role: 'assistant', content: 'hello' }, finish_reason: 'stop' }] }
    }),
  )
  const withTools = await port.chat({
    messages: [{ role: 'user', content: 'hi', tool_calls: undefined }],
    tools: [tool],
  })
  assert.deepEqual(seen[0], { messages: [{ role: 'user', content: 'hi', tool_calls: undefined }], tools: [tool] })
  assert.equal(withTools.choices?.[0]?.message?.content, 'hello')
  // A request with no tools never sends an empty `tools` array.
  await port.chat({ messages: [{ role: 'user', content: 'again' }] })
  assert.deepEqual(seen[1], { messages: [{ role: 'user', content: 'again' }] })
})

test('the service port normalizes an unexpected host reply to no choices', async () => {
  const port = createServiceAiChatPort(fakeService(async () => 'not an object'))
  const reply = await port.chat({ messages: [{ role: 'user', content: 'hi' }] })
  assert.deepEqual(reply, {})
})

test('the service port refuses chat when the host does not implement it', async () => {
  const port = createServiceAiChatPort({} as Pick<WorkbenchService, 'aiConfig' | 'aiChat'>)
  await assert.rejects(() => port.chat({ messages: [{ role: 'user', content: 'hi' }] }), /not available over this host/)
  await assert.rejects(() => port.getConfig(), /not available over this host/)
})
