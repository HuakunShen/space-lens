import * as vscode from 'vscode'
import { createHttpService } from '@space-lens/client'
import { recentRoots, rememberRoot, startMachineService, type RunningService } from './supervisor'
import { createAiBridge, isPathWithin, type AiBridge } from './ai-bridge'
import type { BridgeMessage, BridgeRequest } from './protocol'

const HOST_ROOT = __dirname

interface WorkbenchState {
  panel: vscode.WebviewPanel | null
  service: RunningService | null
  token: string | null
  scanId: string | null
  pollTimer: ReturnType<typeof setInterval> | null
  rootPath: string | null
}

export class Workbench {
  private readonly state: WorkbenchState = {
    panel: null,
    service: null,
    token: null,
    scanId: null,
    pollTimer: null,
    rootPath: null,
  }

  private readonly ai: AiBridge

  /** In-flight `ai.chatStream` requests, keyed by request id. */
  private readonly streams = new Map<number, AbortController>()

  constructor(private readonly context: vscode.ExtensionContext) {
    // The key lives in VS Code's secret store; the rest of the AI config in
    // global state. Both are injected so `ai-bridge` stays free of `vscode`.
    this.ai = createAiBridge({ state: context.globalState, secrets: context.secrets })
  }

  async open(rootPath?: string): Promise<void> {
    const target = rootPath ?? vscode.workspace.workspaceFolders?.[0]?.uri.fsPath
    if (target === undefined) {
      void vscode.window.showErrorMessage('Space Lens: open a folder first, then run “Space Lens: Open Workbench”.')
      return
    }
    if (this.state.rootPath !== null && this.state.rootPath !== target) {
      await this.shutdown()
    }
    this.state.rootPath = target
    rememberRoot(this.context, target)

    if (this.state.panel !== null) {
      this.state.panel.reveal()
      return
    }
    if (this.state.service === null) {
      await this.startService(target)
    }
    this.openPanel(target)
  }

  async openWithPicker(): Promise<void> {
    const workspaceChoices = (vscode.workspace.workspaceFolders ?? []).map((folder) => ({
      label: `$(folder-active) ${folder.name}`,
      description: folder.uri.fsPath,
      picked: folder === vscode.workspace.workspaceFolders?.[0],
      fsPath: folder.uri.fsPath,
    }))
    const recentChoices = recentRoots(this.context)
      .filter((path) => !workspaceChoices.some((choice) => choice.fsPath === path))
      .map((path) => ({ label: `$(history) ${path.split('/').pop()}`, description: path, fsPath: path }))
    const choices = [
      ...workspaceChoices,
      ...recentChoices,
      { label: '$(new-folder) Choose Folder…', description: 'Pick any directory on disk', fsPath: null },
    ]
    const picked = await vscode.window.showQuickPick(choices, { placeHolder: 'Scan which directory?' })
    if (picked === undefined) return
    if (picked.fsPath === null) {
      const dialog = await vscode.window.showOpenDialog({ canSelectFolders: true, canSelectFiles: false, canSelectMany: false })
      if (dialog === undefined || dialog[0] === undefined) return
      await this.open(dialog[0].fsPath)
      return
    }
    await this.open(picked.fsPath)
  }

  async shutdown(): Promise<void> {
    if (this.state.pollTimer !== null) clearInterval(this.state.pollTimer)
    this.state.pollTimer = null
    this.abortStreams()
    this.state.panel?.dispose()
    this.state.panel = null
    this.state.service?.stop()
    this.state.service = null
    this.state.token = null
    this.state.scanId = null
    this.state.rootPath = null
  }

  /** Aborts every in-flight `ai.chatStream`; the owning `answer` settles on its own. */
  private abortStreams(): void {
    for (const controller of this.streams.values()) controller.abort()
    this.streams.clear()
  }

  private async startService(rootPath: string): Promise<void> {
    const cliPath = vscode.workspace.getConfiguration('spacelens').get<string>('cliPath', 'spacelens')
    await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: 'Space Lens: starting local service…' },
      async () => {
        const service = await startMachineService({ cliPath, rootPath })
        // The exchange happens here, in the extension host; the webview never
        // sees the ticket or the token.
        const serviceClient = createHttpService({ baseUrl: service.readiness.url, getToken: () => this.state.token })
        const session = await serviceClient.exchange(service.ticket)
        this.state.service = service
        this.state.token = session.token
      },
    )
  }

  private openPanel(rootPath: string): void {
    if (this.state.service === null) return
    const webviewRoot = vscode.Uri.file(join(HOST_ROOT, 'webview'))
    const panel = vscode.window.createWebviewPanel('spacelens.workbench', `Space Lens — ${rootPath.split('/').pop()}`, vscode.ViewColumn.One, {
      enableScripts: true,
      localResourceRoots: [webviewRoot],
    })
    this.state.panel = panel

    const scriptUri = panel.webview.asWebviewUri(vscode.Uri.file(join(HOST_ROOT, 'webview', 'workbench.js')))
    const cssPath = join(HOST_ROOT, 'webview', 'workbench.css')
    let css = ''
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      css = require('node:fs').readFileSync(cssPath, 'utf8')
    } catch {
      css = ''
    }
    const nonce = Array.from({ length: 32 }, () => Math.floor(Math.random() * 16).toString(16)).join('')
    panel.webview.html = `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${panel.webview.cspSource} 'unsafe-inline'; script-src 'nonce-${nonce}'; img-src ${panel.webview.cspSource} data:; font-src ${panel.webview.cspSource};" />
  <style>${css}</style>
</head>
<body>
  <div id="app"></div>
  <script nonce="${nonce}" src="${scriptUri}"></script>
</body>
</html>`

    panel.webview.onDidReceiveMessage((request: BridgeRequest) => {
      void this.answer(request)
    })
    panel.onDidDispose(() => {
      if (this.state.panel === panel) this.state.panel = null
      // A disposed webview has nowhere to receive chunks; stop the upstream
      // requests rather than stream into the void.
      this.abortStreams()
    })
    this.startPolling()
  }

  private async answer(request: BridgeRequest): Promise<void> {
    if (this.state.panel === null || this.state.service === null) return
    const service = createHttpService({ baseUrl: this.state.service.readiness.url, getToken: () => this.state.token })
    let answer: BridgeMessage
    try {
      switch (request.kind) {
        case 'pair': {
          const capabilities = await service.capabilities()
          const roots = await service.roots()
          answer = { id: request.id, ok: true, answer: { kind: 'pair', scanId: null, capabilities, roots } as never }
          break
        }
        case 'scan.start': {
          const session = await service.startScan({
            paths: request.paths ?? [],
            ignoreHidden: false,
            respectGitignore: true,
            ignoredMode: 'summarize',
            label: request.paths?.[0],
          })
          this.state.scanId = session.scanId
          answer = { id: request.id, ok: true, answer: { kind: 'scan.start', session } }
          break
        }
        case 'scan.status': {
          answer = { id: request.id, ok: true, answer: { kind: 'scan.status', status: await service.scanStatus(request.scanId ?? '') } }
          break
        }
        case 'tree.slice': {
          const slice = await service.treeSlice({
            scanId: request.scanId ?? '',
            nodeId: request.nodeId ?? '',
            depth: request.depth ?? 3,
            maxChildrenPerNode: request.maxChildrenPerNode ?? 50,
          })
          answer = { id: request.id, ok: true, answer: { kind: 'tree.slice', slice } }
          break
        }
        case 'tree.children': {
          const page = await service.children({
            scanId: request.scanId ?? '',
            nodeId: request.nodeId ?? '',
            offset: request.offset ?? 0,
            limit: request.limit ?? 200,
            sort: request.sort ?? 'size',
          })
          answer = { id: request.id, ok: true, answer: { kind: 'tree.children', page } }
          break
        }
        case 'ai.config': {
          answer = { id: request.id, ok: true, answer: { kind: 'ai.config', config: await this.ai.getConfig() } }
          break
        }
        case 'ai.setConfig': {
          answer = { id: request.id, ok: true, answer: { kind: 'ai.config', config: await this.ai.setConfig(request.ai ?? {}) } }
          break
        }
        case 'ai.chat': {
          const completion = await this.ai.chat({
            messages: (request.messages ?? []) as Record<string, unknown>[],
            tools: request.tools as Record<string, unknown>[] | undefined,
          })
          answer = { id: request.id, ok: true, answer: { kind: 'ai.chat', completion } }
          break
        }
        case 'ai.chatStream': {
          // The request id doubles as the run id: chunks are tagged with it and
          // `ai.cancel` names it. Rejecting a duplicate keeps one controller per id.
          if (this.streams.has(request.id)) {
            answer = {
              id: request.id,
              ok: false,
              problem: { code: 'InvalidRequest', message: `A stream with id ${request.id} is already running.` },
            }
            break
          }
          const controller = new AbortController()
          this.streams.set(request.id, controller)
          try {
            await this.ai.chatStream({
              messages: (request.messages ?? []) as Record<string, unknown>[],
              tools: request.tools as Record<string, unknown>[] | undefined,
              signal: controller.signal,
              onChunk: (text) => {
                const panel = this.state.panel
                if (panel === null) return
                void panel.webview.postMessage({ kind: 'ai.chunk', id: request.id, text } satisfies BridgeMessage)
              },
            })
            answer = { id: request.id, ok: true, answer: { kind: 'ai.chatStream', done: true } }
          } finally {
            this.streams.delete(request.id)
          }
          break
        }
        case 'ai.cancel': {
          const runId = request.runId
          const controller = typeof runId === 'number' ? this.streams.get(runId) : undefined
          controller?.abort()
          // Cancelling an already-finished (or unknown) run is a no-op, not an error.
          answer = { id: request.id, ok: true, answer: { kind: 'ai.cancel', cancelled: controller !== undefined } }
          break
        }
        case 'reveal': {
          if (!(await this.reveal(request.path))) {
            answer = {
              id: request.id,
              ok: false,
              problem: { code: 'InvalidRequest', message: 'Refusing to reveal a path outside the scanned root.' },
            }
            break
          }
          answer = { id: request.id, ok: true, answer: { kind: 'reveal', revealed: true } }
          break
        }
        default:
          answer = { id: request.id, ok: false, problem: { code: 'UnsupportedOperation', message: `unknown kind` } }
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      answer = { id: request.id, ok: false, problem: { code: 'Unavailable', message } }
    }
    // The panel can be disposed while a stream is in flight; drop the answer
    // rather than dereferencing a null panel.
    void this.state.panel?.webview.postMessage(answer)
  }

  /**
   * Reveals `path` in the OS file manager, but only when it is the scanned root
   * or a path inside it and it actually exists. The sandboxed webview can name
   * any path; this is the one gate between that name and the OS.
   */
  private async reveal(path: string | undefined): Promise<boolean> {
    if (typeof path !== 'string') return false
    if (this.state.rootPath === null) return false
    if (!isPathWithin(this.state.rootPath, path)) return false
    let exists = false
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      exists = require('node:fs').existsSync(path)
    } catch {
      exists = false
    }
    if (!exists) return false
    await vscode.commands.executeCommand('revealFileInOS', vscode.Uri.file(path))
    return true
  }

  /** Clears the stored AI API key; used by the `spacelens.clearAiKey` command. */
  async clearAiKey(): Promise<void> {
    await this.ai.clearKey()
  }

  private startPolling(): void {
    if (this.state.pollTimer !== null) return
    // The machine-mode host exposes no per-path progress; poll status while a
    // scan is running and push updates into the webview.
    this.state.pollTimer = setInterval(() => {
      if (this.state.panel === null || this.state.service === null || this.state.scanId === null) return
      const service = createHttpService({ baseUrl: this.state.service.readiness.url, getToken: () => this.state.token })
      void service
        .scanStatus(this.state.scanId)
        .then((status) => {
          if (status.state !== 'scanning') return
          void this.state.panel?.webview.postMessage({ kind: 'push', status } satisfies BridgeMessage)
        })
        .catch(() => {})
    }, 1_500)
  }
}

import { join } from 'node:path'

export function activate(context: vscode.ExtensionContext): void {
  const workbench = new Workbench(context)

  const openItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 50)
  openItem.text = '$(diff-multiple) Space Lens'
  openItem.command = 'spacelens.openWorkbench'
  openItem.tooltip = 'Open the Space Lens workbench'
  openItem.show()

  context.subscriptions.push(
    openItem,
    vscode.commands.registerCommand('spacelens.openWorkbench', () => {
      void workbench.open()
    }),
    vscode.commands.registerCommand('spacelens.chooseDirectory', () => {
      void workbench.openWithPicker()
    }),
    vscode.commands.registerCommand('spacelens.shutdown', () => {
      void workbench.shutdown()
    }),
    vscode.commands.registerCommand('spacelens.clearAiKey', async () => {
      await workbench.clearAiKey()
      void vscode.window.showInformationMessage('Space Lens: AI API key cleared.')
    }),
    new vscode.Disposable(() => {
      void workbench.shutdown()
    }),
  )
}

export function deactivate(): void {}
