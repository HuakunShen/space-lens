import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { TauriPorts } from '@space-lens/client'
import { createAiChatPort } from '../src/lib/ai/chat-port.ts'

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
  assert.deepEqual(calls[0], { cmd: 'ai_set_config', args: { baseUrl: 'https://x/v1', model: 'm', disableThinking: true } })
  await port.setConfig({ baseUrl: 'https://x/v1', model: 'm' })
  assert.ok(!('disableThinking' in (calls[1]!.args as object)))
})

test('a stale host reply without the field reads as thinking enabled', async () => {
  const { ports } = fakePorts({ baseUrl: 'u', model: 'm', hasKey: false })
  const config = await createAiChatPort(async () => ports).getConfig()
  assert.equal(config.disableThinking, false)
})
