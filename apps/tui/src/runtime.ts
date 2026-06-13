export interface RuntimeGlobal {
  Bun?: unknown
}

export function isBunRuntime(runtime: RuntimeGlobal = globalThis as RuntimeGlobal): boolean {
  return runtime.Bun !== undefined
}

export function assertSupportedRuntime(runtime: RuntimeGlobal = globalThis as RuntimeGlobal): void {
  if (!isBunRuntime(runtime)) {
    throw new Error('OpenTUI currently requires Bun. Run this CLI with `bun` or `yarn tui`.')
  }
}
