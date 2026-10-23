import { expect, it, vi } from 'vitest'
import { createTauriService, TauriProblemError, type TauriPorts } from '../src/tauri.ts'

const status = {
  scanId: 'scan_abcdefgh',
  state: 'scanning',
  message: '',
  progress: null,
  currentPath: '/tmp/local',
  bytesScanned: 4096,
  entriesScanned: 2,
  rootIds: [],
  label: 'local',
  updatedAt: '2026-10-02T08:00:00.000Z',
}

it.each([true, false, undefined])(
  'forwards localOnly=%s without defaulting protected IPC to legacy',
  async (localOnly) => {
    let receive: (message: unknown) => void = () => {}
    const requests: Record<string, unknown>[] = []
    const ports: TauriPorts = {
      createChannel(callback) {
        receive = callback
        return 'channel'
      },
      async invoke(command, payload) {
        if (command === 'sl_connect') return { sessionId: 'session_1' }
        requests.push(payload!.request as Record<string, unknown>)
        receive({
          ok: true,
          result: { scanId: 'scan_abcdefgh', rootIds: [], createdAt: '2026-10-02T08:00:00.000Z', label: 'local' },
        })
      },
    }
    await createTauriService(ports).startScan({
      paths: ['/tmp/local'],
      ignoreHidden: false,
      respectGitignore: true,
      ignoredMode: 'summarize',
      ...(localOnly === undefined ? {} : { localOnly }),
    })
    expect(requests[0]).toEqual({
      kind: 'scanStart',
      paths: ['/tmp/local'],
      ignoreHidden: false,
      respectGitignore: true,
      ignoredMode: 'summarize',
      label: null,
      ...(localOnly === undefined ? {} : { localOnly }),
    })
  },
)

it('restores and validates native scan lists through the existing window session', async () => {
  let receive: (message: unknown) => void = () => {}
  const requests: unknown[] = []
  let response: unknown = [status]
  const ports: TauriPorts = {
    createChannel(callback) {
      receive = callback
      return 'channel'
    },
    async invoke(command, payload) {
      if (command === 'sl_connect') return { sessionId: 'session_1' }
      requests.push({ command, ...payload })
      receive({ ok: true, result: response })
    },
  }
  const service = createTauriService(ports)
  expect(await service.listScans!()).toEqual([status])
  expect(requests[0]).toEqual({
    command: 'sl_read',
    sessionId: 'session_1',
    reply: 'channel',
    request: { method: 'scanList' },
  })
  response = [{ ...status, bytesScanned: -1 }]
  await expect(service.listScans!()).rejects.toThrow()
})

it('sends closed discovery IPC requests and receives paginated results through the channel', async () => {
  let receive: (message: unknown) => void = () => {}
  const requests: unknown[] = []
  const page = { scanId: 'scan_1', kind: 'large-files', items: [], total: 3, totalSize: 900, offset: 1, limit: 2 }
  const ports: TauriPorts = {
    createChannel(callback) {
      receive = callback
      return 'channel'
    },
    async invoke(command, payload) {
      if (command === 'sl_connect') return { sessionId: 'session_1' }
      requests.push({ command, ...payload })
      receive({ ok: true, result: page })
      return undefined
    },
  }
  const service = createTauriService(ports)
  expect(await service.discover!({ scanId: 'scan_1', kind: 'large-files', minSize: 100, offset: 1, limit: 2 })).toEqual(
    page,
  )
  expect(requests).toEqual([
    {
      command: 'sl_read',
      sessionId: 'session_1',
      reply: 'channel',
      request: { method: 'discovery', scanId: 'scan_1', kind: 'large-files', minSize: 100, offset: 1, limit: 2 },
    },
  ])
})

it('preserves typed discovery failures from the native nested problem envelope', async () => {
  let receive: (message: unknown) => void = () => {}
  const ports: TauriPorts = {
    createChannel(callback) {
      receive = callback
      return 'channel'
    },
    async invoke(command) {
      if (command === 'sl_connect') return { sessionId: 'session_1' }
      receive({ ok: false, problem: { problem: { code: 'StaleSnapshot', message: 'scan again', retryable: false } } })
      return undefined
    },
  }
  const result = await createTauriService(ports).discover!({
    scanId: 'scan_1',
    kind: 'caches',
    minSize: 0,
    offset: 0,
    limit: 200,
  }).catch((error: unknown) => error)
  expect(result).toBeInstanceOf(TauriProblemError)
  expect(result).toMatchObject({ code: 'StaleSnapshot', message: 'scan again' })
})

it('sends closed app cleanup plan/execute IPC requests through the channel', async () => {
  let receive: (message: unknown) => void = () => {}
  const requests: unknown[] = []
  const plan = {
    planId: 'plan_abcdef1234567890',
    auditId: 'audit_abcdef1234567890abcdef',
    appId: 'codex-cli',
    mode: 'trash',
    entries: [],
    totalSize: 0,
    errors: [],
    createdAt: '2026-10-13T00:00:00.000Z',
    expiresAt: '2026-10-13T00:10:00.000Z',
  }
  const outcome = { trashed: [], bytesFreed: 0, failed: [] }
  const results: unknown[] = [plan, outcome]
  const ports: TauriPorts = {
    createChannel(callback) {
      receive = callback
      return 'channel'
    },
    async invoke(command, payload) {
      if (command === 'sl_connect') return { sessionId: 'session_1' }
      requests.push({ command, ...payload })
      receive({ ok: true, result: results[requests.length - 1] })
      return undefined
    },
  }
  const service = createTauriService(ports)
  expect(
    await service.appCleanupPlan!({
      auditId: 'audit_abcdef1234567890abcdef',
      appId: 'codex-cli',
      paths: ['/tmp/a'],
    }),
  ).toEqual(plan)
  expect(await service.appCleanupExecute!({ planId: 'plan_abcdef1234567890', confirm: true })).toEqual(outcome)
  expect(requests).toEqual([
    {
      command: 'sl_submit',
      sessionId: 'session_1',
      reply: 'channel',
      request: {
        kind: 'appCleanupPlan',
        auditId: 'audit_abcdef1234567890abcdef',
        appId: 'codex-cli',
        paths: ['/tmp/a'],
      },
    },
    {
      command: 'sl_submit',
      sessionId: 'session_1',
      reply: 'channel',
      request: { kind: 'appCleanupExecute', planId: 'plan_abcdef1234567890', confirm: true },
    },
  ])
})

it('sends closed reveal IPC requests and preserves typed app cleanup failures', async () => {
  let receive: (message: unknown) => void = () => {}
  const requests: unknown[] = []
  const ports: TauriPorts = {
    createChannel(callback) {
      receive = callback
      return 'channel'
    },
    async invoke(command, payload) {
      if (command === 'sl_connect') return { sessionId: 'session_1' }
      requests.push({ command, ...payload })
      if (requests.length === 1) receive({ ok: true, result: { revealed: true } })
      else
        receive({
          ok: false,
          problem: { problem: { code: 'StalePlan', message: 'plan expired', retryable: false } },
        })
      return undefined
    },
  }
  const service = createTauriService(ports)
  expect(await service.revealAppLocation!({ appId: 'codex-cli', path: '/tmp/a' })).toEqual({ revealed: true })
  expect(requests[0]).toEqual({
    command: 'sl_host_request',
    sessionId: 'session_1',
    reply: 'channel',
    request: { kind: 'revealPath', appId: 'codex-cli', path: '/tmp/a' },
  })
  const failure = await service
    .appCleanupExecute!({ planId: 'plan_abcdef1234567890', confirm: true })
    .catch((error: unknown) => error)
  expect(failure).toBeInstanceOf(TauriProblemError)
  expect(failure).toMatchObject({ code: 'StalePlan', message: 'plan expired' })
})

it('sends the scanned-path reveal as its own host request arm', async () => {
  let receive: (message: unknown) => void = () => {}
  const requests: unknown[] = []
  const ports: TauriPorts = {
    createChannel(callback) {
      receive = callback
      return 'channel'
    },
    async invoke(command, payload) {
      if (command === 'sl_connect') return { sessionId: 'session_1' }
      requests.push({ command, ...payload })
      receive({ ok: true, result: { revealed: true } })
      return undefined
    },
  }
  const service = createTauriService(ports)
  expect(await service.revealPath!({ path: '/tmp/a' })).toEqual({ revealed: true })
  expect(requests[0]).toEqual({
    command: 'sl_host_request',
    sessionId: 'session_1',
    reply: 'channel',
    request: { kind: 'revealScanPath', path: '/tmp/a', action: 'reveal' },
  })
})

it('sends the exact appScanStart submit arm and returns the app session', async () => {
  let receive: (message: unknown) => void = () => {}
  const requests: unknown[] = []
  const session = {
    scanId: 'scan_app0001',
    rootIds: ['approot_abcdef1234567890abcdef'],
    createdAt: '2026-10-13T00:00:00.000Z',
    label: 'Raycast',
    app: { appId: 'raycast', appName: 'Raycast', auditId: 'audit_abcdef1234567890abcdef' },
  }
  const ports: TauriPorts = {
    createChannel(callback) {
      receive = callback
      return 'channel'
    },
    async invoke(command, payload) {
      if (command === 'sl_connect') return { sessionId: 'session_1' }
      requests.push({ command, ...payload })
      receive({ ok: true, result: session })
      return undefined
    },
  }
  expect(
    await createTauriService(ports).startAppScan!({
      auditId: 'audit_abcdef1234567890abcdef',
      appId: 'raycast',
    }),
  ).toEqual(session)
  expect(requests).toEqual([
    {
      command: 'sl_submit',
      sessionId: 'session_1',
      reply: 'channel',
      request: { kind: 'appScanStart', auditId: 'audit_abcdef1234567890abcdef', appId: 'raycast' },
    },
  ])
})

it('keeps an app scan start pending beyond the ordinary one-minute reply timeout', async () => {
  vi.useFakeTimers()
  try {
    let receive: (message: unknown) => void = () => {}
    const ports: TauriPorts = {
      createChannel(callback) {
        receive = callback
        return 'channel'
      },
      async invoke(command) {
        return command === 'sl_connect' ? { sessionId: 'session_1' } : undefined
      },
    }
    const session = {
      scanId: 'scan_app0001',
      rootIds: ['approot_abcdef1234567890abcdef'],
      createdAt: '2026-10-13T00:00:00.000Z',
      label: 'Raycast',
    }
    const pending = createTauriService(ports)
      .startAppScan!({ auditId: 'audit_abcdef1234567890abcdef', appId: 'raycast' })
      .catch((error: unknown) => error)
    await vi.advanceTimersByTimeAsync(61_000)
    receive({ ok: true, result: session })
    expect(await pending).toEqual(session)
  } finally {
    vi.useRealTimers()
  }
})

it('keeps a background discovery request pending beyond the ordinary one-minute reply timeout', async () => {
  vi.useFakeTimers()
  try {
    let receive: (message: unknown) => void = () => {}
    const ports: TauriPorts = {
      createChannel(callback) {
        receive = callback
        return 'channel'
      },
      async invoke(command) {
        return command === 'sl_connect' ? { sessionId: 'session_1' } : undefined
      },
    }
    const page = { scanId: 'scan_1', kind: 'caches', items: [], total: 0, totalSize: 0, offset: 0, limit: 200 }
    const pending = createTauriService(ports).discover!({
      scanId: 'scan_1',
      kind: 'caches',
      minSize: 0,
      offset: 0,
      limit: 200,
    }).catch((error: unknown) => error)
    await vi.advanceTimersByTimeAsync(61_000)
    receive({ ok: true, result: page })
    expect(await pending).toEqual(page)
  } finally {
    vi.useRealTimers()
  }
})
