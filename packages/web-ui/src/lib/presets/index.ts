import { AI_PRESET } from './ai.ts'
import type { SoftwarePreset } from './software.ts'

export * from './software.ts'
export { AI_PRESET } from './ai.ts'

/** Every preset the storage and memory views offer. Append new presets here. */
export const SOFTWARE_PRESETS: readonly SoftwarePreset[] = [AI_PRESET]
