/**
 * @space-lens/ai-advisor — the host-free AI advisor core.
 *
 * One barrel so hosts (the web app, the DSH plugin, the VS Code webview) can
 * import the whole feature from a single entry: the agent loop (`runAgent`),
 * the read-only tool layer and its `LensPort` adapter (`lensPortFromService`),
 * the system prompt and presets, the collector stager, the safe markdown
 * renderer and the two ports (`WorkbenchService` from `@space-lens/client`,
 * `AiChatPort` from `./ports.ts`) a host must supply.
 *
 * Everything here is host-free: no DOM, Tauri or filesystem import lives in
 * this package. A host injects its own `LensPort` and `ChatPort`; the concrete
 * adapters (for example `createAiChatPort` over Tauri) stay with the host.
 */
export * from './agent.ts'
export * from './markdown.ts'
export * from './ports.ts'
export * from './prompts.ts'
export * from './stage.ts'
export * from './tools.ts'
export * from './types.ts'
