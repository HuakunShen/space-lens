import type { SoftwarePreset } from './software.ts'

/**
 * "AI software": what AI assistants, coding agents and local model runners
 * cost on disk and in memory. Add a program by appending one rule; see
 * `CONTRIBUTING.md` next to this file.
 */
export const AI_PRESET: SoftwarePreset = {
  id: 'ai',
  label: 'AI software',
  description: 'Assistants, coding agents and local model runners.',
  rules: [
    {
      id: 'chatgpt',
      name: 'ChatGPT',
      storage: [],
      match: { commands: [/\/ChatGPT\.app\//] },
    },
    {
      id: 'claude-desktop',
      name: 'Claude',
      storage: ['claude-desktop'],
      match: { commands: [/\/Claude\.app\//] },
    },
    {
      id: 'claude-code',
      name: 'Claude Code',
      storage: ['claude-code-cli'],
      match: { names: ['claude'], commands: [/\/claude-code\//] },
    },
    {
      id: 'codex',
      name: 'Codex',
      storage: ['codex-cli'],
      match: { names: ['codex'], commands: [/@openai\/codex\//, /\/\.codex\//] },
    },
    {
      id: 'cursor',
      name: 'Cursor',
      storage: ['cursor'],
      match: { commands: [/\/Cursor\.app\//] },
    },
    {
      id: 'ollama',
      name: 'Ollama',
      storage: ['ollama'],
      match: { names: ['ollama'], commands: [/\/Ollama\.app\//] },
    },
    {
      id: 'gemini-cli',
      name: 'Gemini CLI',
      storage: ['gemini-cli'],
      match: { commands: [/@google\/gemini-cli\//] },
    },
    {
      id: 'opencode',
      name: 'OpenCode',
      storage: ['opencode-cli'],
      match: { names: ['opencode'] },
    },
    {
      id: 'kimi-code',
      name: 'Kimi Code',
      storage: ['kimi-code'],
      match: { commands: [/\/kimi-code\//] },
    },
    {
      id: 'caveman',
      name: 'Caveman',
      storage: ['caveman'],
      match: { commands: [/\/\.caveman\//] },
    },
  ],
}
