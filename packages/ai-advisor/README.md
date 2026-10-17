# @space-lens/ai-advisor

The host-free core of the Space Lens AI advisor: an agent loop that reads one
scan through bounded, read-only tools and **stages** deletion candidates into
the existing Collector for the user to review. It never deletes anything.

Extracted from `apps/web` so other hosts — the web app, the [DSH](../../apps/dsh)
plugin, the VS Code webview — reuse the same logic and prompts instead of
forking them.

## What's here

- `agent.ts` — `runAgent`, the tool-calling loop with the token/round budget and
  context compaction.
- `tools.ts` — the four tool schemas and handlers, the run-scoped guards, and
  `lensPortFromService`, which adapts a `WorkbenchService` onto `LensPort`.
- `types.ts` — the wire shapes (`ChatMessage`, …) and `LensPort`.
- `prompts.ts` — `SYSTEM_PROMPT` and `PRESETS`.
- `stage.ts` — `createStager`, the bridge to the existing collector.
- `markdown.ts` — `renderMarkdown`, the minimal escaping renderer.
- `ports.ts` — the host config port (`AiConfig`, `AiConfigInput`, `AiChatPort`).
- `index.ts` — the barrel every host imports from.

## The two ports

A host supplies exactly two capabilities; the core imports no host API:

1. **`WorkbenchService`** (from `@space-lens/client`) — the read-only scan
   service. `lensPortFromService` maps it onto `LensPort` for one scan; `stage`
   is supplied by the caller (the collector owns the only side effect).
2. **`AiChatPort`** (`./ports.ts`) — one non-streaming completion plus the
   config surface (`getConfig` / `setConfig`). The web app implements it two
   ways: over Tauri (`invoke('ai_chat')`, BYOK and editable) and over the HTTP
   host's managed routes (`managed: true`, config read-only).

See [`docs/ai-advisor-plan.md`](../../docs/ai-advisor-plan.md) for the full design.

## Tooling

- `node --test 'test/*.test.ts'` — unit tests (one opt-in live test).
- `tsc --noEmit` — typecheck (also wired as `lint`).
