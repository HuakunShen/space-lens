// Unit tests for the extension-host AI bridge (task E). No `vscode`: the state,
// secret store and fetch are all fakes, so the transport behaviour can be
// exercised without a running extension host or a real model.
import { strict as assert } from 'node:assert'
import { test } from 'node:test'

import {
  AI_CONFIG_KEY,
  AI_SECRET_KEY,
  DEFAULT_AI_BASE_URL,
  createAiBridge,
  isPathWithin,
  type AiSecretStore,
  type AiStateStore,
} from '../src/ai-bridge.ts'

const KEY = 'sk-test-secret-value'
const LOOPBACK_BASE = 'http://127.0.0.1:1234/v1'

// ------------------------------------------------------------------ fakes

interface FakeState extends AiStateStore {
  data: Map<string, unknown>
}

function makeState(): FakeState {
  const data = new Map<string, unknown>()
  return {
    data,
    get<T>(key: string): T | undefined {
      return data.get(key) as T | undefined
    },
    async update(key: string, value: unknown): Promise<void> {
      data.set(key, value)
    },
  }
}

interface FakeSecrets extends AiSecretStore {
  data: Map<string, string>
}

function makeSecrets(): FakeSecrets {
  const data = new Map<string, string>()
  return {
    data,
    async get(key: string): Promise<string | undefined> {
      return data.get(key)
    },
    async store(key: string, value: string): Promise<void> {
      data.set(key, value)
    },
    async delete(key: string): Promise<void> {
      data.delete(key)
    },
  }
}

interface CapturedRequest {
  url: string
  headers: Record<string, string>
  body: unknown
}

function bodyText(body: BodyInit | null | undefined): string {
  if (typeof body === 'string') return body
  if (body instanceof Uint8Array) return new TextDecoder().decode(body)
  if (body instanceof ArrayBuffer) return new TextDecoder().decode(body)
  return ''
}

interface FakeFetch {
  impl: typeof fetch
  requests: CapturedRequest[]
}

/** A fetch that never touches the network; `respond` decides each reply. */
function makeFetch(respond: (request: CapturedRequest) => Response | Promise<Response>): FakeFetch {
  const requests: CapturedRequest[] = []
  const impl: typeof fetch = async (input, init) => {
    const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url
    const request: CapturedRequest = {
      url,
      headers: { ...((init?.headers as Record<string, string> | undefined) ?? {}) },
      body: JSON.parse(bodyText(init?.body)) as unknown,
    }
    requests.push(request)
    return respond(request)
  }
  return { impl, requests }
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })
}

const neverFetch: typeof fetch = async () => {
  throw new Error('fetch must not be called')
}

// ------------------------------------------------------------------ config

test('getConfig returns the documented defaults and no key', async () => {
  const bridge = createAiBridge({ state: makeState(), secrets: makeSecrets() })
  assert.deepEqual(await bridge.getConfig(), {
    baseUrl: DEFAULT_AI_BASE_URL,
    model: '',
    hasKey: false,
    disableThinking: false,
    managed: false,
  })
})

test('setConfig round trip never exposes the key', async () => {
  const state = makeState()
  const secrets = makeSecrets()
  const bridge = createAiBridge({ state, secrets })

  const config = await bridge.setConfig({
    baseUrl: LOOPBACK_BASE,
    model: 'llama3',
    apiKey: `  ${KEY}  `,
    disableThinking: true,
  })

  assert.deepEqual(config, {
    baseUrl: LOOPBACK_BASE,
    model: 'llama3',
    hasKey: true,
    disableThinking: true,
    managed: false,
  })
  // The returned config must not carry the key, in any shape.
  assert.equal('apiKey' in config, false)
  assert.equal(JSON.stringify(config).includes(KEY), false)
  // The key is not in the non-secret state either.
  assert.equal(JSON.stringify([...state.data.values()]).includes(KEY), false)
  assert.deepEqual(state.data.get(AI_CONFIG_KEY), { baseUrl: LOOPBACK_BASE, model: 'llama3', disableThinking: true })
  // It is stored, trimmed, in secrets only.
  assert.equal(secrets.data.get(AI_SECRET_KEY), KEY)
})

test('apiKey undefined keeps, null clears, blank clears, a string replaces (trimmed)', async () => {
  const secrets = makeSecrets()
  const bridge = createAiBridge({ state: makeState(), secrets })

  await bridge.setConfig({ baseUrl: LOOPBACK_BASE, model: 'm', apiKey: ` ${KEY} ` })
  assert.equal(secrets.data.get(AI_SECRET_KEY), KEY)

  // undefined (or an omitted field) keeps the stored key and the rest of config.
  const kept = await bridge.setConfig({ model: 'm2' })
  assert.equal(secrets.data.get(AI_SECRET_KEY), KEY)
  assert.equal(kept.baseUrl, LOOPBACK_BASE)
  assert.equal(kept.model, 'm2')
  assert.equal(kept.hasKey, true)

  // A blank string clears.
  await bridge.setConfig({ apiKey: '   ' })
  assert.equal(secrets.data.has(AI_SECRET_KEY), false)
  assert.equal((await bridge.getConfig()).hasKey, false)

  // null clears too.
  await bridge.setConfig({ apiKey: KEY })
  assert.equal((await bridge.getConfig()).hasKey, true)
  await bridge.setConfig({ apiKey: null })
  assert.equal(secrets.data.has(AI_SECRET_KEY), false)
  assert.equal((await bridge.getConfig()).hasKey, false)
})

test('clearKey deletes the secret and keeps the non-secret config', async () => {
  const state = makeState()
  const secrets = makeSecrets()
  const bridge = createAiBridge({ state, secrets })
  await bridge.setConfig({ baseUrl: LOOPBACK_BASE, model: 'm', apiKey: KEY })

  await bridge.clearKey()

  assert.equal(secrets.data.has(AI_SECRET_KEY), false)
  const config = await bridge.getConfig()
  assert.equal(config.hasKey, false)
  assert.equal(config.baseUrl, LOOPBACK_BASE)
  assert.equal(config.model, 'm')
})

test('setConfig rejects an invalid base URL with a clear message and stores nothing', async () => {
  const state = makeState()
  const bridge = createAiBridge({ state, secrets: makeSecrets() })

  for (const baseUrl of ['http://example.com/v1', 'not a url', 'ftp://example.com/v1', '']) {
    await assert.rejects(() => bridge.setConfig({ baseUrl }), /AI base URL is invalid/, baseUrl)
  }
  assert.equal(state.data.size, 0)
})

test('setConfig accepts https and loopback http base URLs', async () => {
  const bridge = createAiBridge({ state: makeState(), secrets: makeSecrets() })
  assert.equal((await bridge.setConfig({ baseUrl: 'https://api.openai.com/v1' })).baseUrl, 'https://api.openai.com/v1')
  // The stored value is the trimmed input; `chat` derives the endpoint, so a
  // trailing slash is harmless and preserved as typed.
  assert.equal((await bridge.setConfig({ baseUrl: '  http://localhost:8080/v1/  ' })).baseUrl, 'http://localhost:8080/v1/')
})

// ------------------------------------------------------------------ chat

test('chat posts to the endpoint derived from the stored base URL, with Bearer from secrets', async () => {
  const secrets = makeSecrets()
  const { impl, requests } = makeFetch(() => jsonResponse({ id: 'cmpl', choices: [] }))
  const bridge = createAiBridge({ state: makeState(), secrets, fetchImpl: impl })
  await bridge.setConfig({ baseUrl: LOOPBACK_BASE, model: 'llama3', apiKey: KEY })

  const result = await bridge.chat({ messages: [{ role: 'user', content: 'hi' }] })

  assert.deepEqual(result, { id: 'cmpl', choices: [] })
  assert.equal(requests.length, 1)
  assert.equal(requests[0].url, `${LOOPBACK_BASE}/chat/completions`)
  assert.equal(requests[0].headers.authorization, `Bearer ${KEY}`)
  assert.deepEqual(requests[0].body, { model: 'llama3', messages: [{ role: 'user', content: 'hi' }] })
})

test('chat sends no Authorization header when no key is stored', async () => {
  const { impl, requests } = makeFetch(() => jsonResponse({ choices: [] }))
  const bridge = createAiBridge({ state: makeState(), secrets: makeSecrets(), fetchImpl: impl })
  await bridge.setConfig({ baseUrl: LOOPBACK_BASE, model: 'llama3' })

  await bridge.chat({ messages: [] })

  assert.equal('authorization' in requests[0].headers, false)
})

test('chat sends the thinking flag only when enabled, and forwards tools when non-empty', async () => {
  const enabled = makeFetch(() => jsonResponse({ choices: [] }))
  const bridge = createAiBridge({ state: makeState(), secrets: makeSecrets(), fetchImpl: enabled.impl })
  await bridge.setConfig({ baseUrl: LOOPBACK_BASE, model: 'm', disableThinking: true })
  await bridge.chat({ messages: [{ role: 'user', content: 'hi' }], tools: [{ type: 'function' }] })
  assert.deepEqual(enabled.requests[0].body, {
    model: 'm',
    messages: [{ role: 'user', content: 'hi' }],
    thinking: { type: 'disabled' },
    tools: [{ type: 'function' }],
  })

  const disabled = makeFetch(() => jsonResponse({ choices: [] }))
  const bridge2 = createAiBridge({ state: makeState(), secrets: makeSecrets(), fetchImpl: disabled.impl })
  await bridge2.setConfig({ baseUrl: LOOPBACK_BASE, model: 'm' })
  await bridge2.chat({ messages: [], tools: [] })
  assert.deepEqual(disabled.requests[0].body, { model: 'm', messages: [] })
})

test('chat redacts the key in an error response body', async () => {
  const secrets = makeSecrets()
  const { impl } = makeFetch(() => new Response(`invalid api key: ${KEY}`, { status: 401 }))
  const bridge = createAiBridge({ state: makeState(), secrets, fetchImpl: impl })
  await bridge.setConfig({ baseUrl: LOOPBACK_BASE, model: 'm', apiKey: KEY })

  await assert.rejects(
    () => bridge.chat({ messages: [] }),
    (error: Error) => {
      assert.equal(error.message.includes(KEY), false, error.message)
      assert.ok(error.message.includes('[redacted]'), error.message)
      return true
    },
  )
})

test('chat redacts the key in a transport error', async () => {
  const secrets = makeSecrets()
  const { impl } = makeFetch(() => {
    throw new Error(`connect ECONNREFUSED while sending ${KEY}`)
  })
  const bridge = createAiBridge({ state: makeState(), secrets, fetchImpl: impl })
  await bridge.setConfig({ baseUrl: LOOPBACK_BASE, model: 'm', apiKey: KEY })

  await assert.rejects(
    () => bridge.chat({ messages: [] }),
    (error: Error) => {
      assert.equal(error.message.includes(KEY), false, error.message)
      return true
    },
  )
})

test('chat refuses when no model is set, without calling fetch', async () => {
  const bridge = createAiBridge({ state: makeState(), secrets: makeSecrets(), fetchImpl: neverFetch })
  await assert.rejects(() => bridge.chat({ messages: [] }), /AI model is not set/)
})

test('chat refuses when the model is only whitespace', async () => {
  const bridge = createAiBridge({ state: makeState(), secrets: makeSecrets(), fetchImpl: neverFetch })
  await bridge.setConfig({ model: '   ' })
  await assert.rejects(() => bridge.chat({ messages: [] }), /AI model is not set/)
})

// ------------------------------------------------------------------ isPathWithin

test('isPathWithin accepts the root and its descendants', () => {
  assert.equal(isPathWithin('/a/b', '/a/b'), true)
  assert.equal(isPathWithin('/a/b', '/a/b/c'), true)
  assert.equal(isPathWithin('/a/b', '/a/b/c/d/e'), true)
  assert.equal(isPathWithin('/a/b/', '/a/b/c'), true)
  assert.equal(isPathWithin('/a/b', '/a/b/'), true)
  assert.equal(isPathWithin('/', '/etc/hosts'), true)
})

test('isPathWithin rejects escapes, siblings and unrelated paths', () => {
  // sibling sharing a name prefix: /a/bc is not inside /a/b
  assert.equal(isPathWithin('/a/b', '/a/bc'), false)
  assert.equal(isPathWithin('/a/b', '/a/bc/d'), false)
  // `..` escape normalized away
  assert.equal(isPathWithin('/a/b', '/a'), false)
  assert.equal(isPathWithin('/a/b', '/a/b/../c'), false)
  assert.equal(isPathWithin('/a/b', '/a/b/../../etc/passwd'), false)
  // unrelated
  assert.equal(isPathWithin('/a/b', '/x/y'), false)
})
