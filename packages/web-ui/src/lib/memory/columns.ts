// Resizable columns for the process table and tree, Activity Monitor style:
// the name column has its own width instead of absorbing every spare pixel,
// so the numbers sit next to the names, and every column can be dragged.
// Widths persist per view in localStorage (a per-viewer convenience; a
// blocked or corrupt store just yields the defaults).

export interface ColumnSpec {
  defaultWidth: number
  min: number
  max: number
}

export type ColumnWidths<K extends string> = Record<K, number>

export const TABLE_COLUMNS = {
  name: { defaultWidth: 300, min: 140, max: 720 },
  cpu: { defaultWidth: 64, min: 48, max: 160 },
  memory: { defaultWidth: 96, min: 64, max: 200 },
  real: { defaultWidth: 96, min: 64, max: 200 },
  virtual: { defaultWidth: 96, min: 64, max: 200 },
  pid: { defaultWidth: 72, min: 52, max: 140 },
  kind: { defaultWidth: 80, min: 56, max: 160 },
} as const satisfies Record<string, ColumnSpec>

export const TREE_COLUMNS = {
  name: { defaultWidth: 340, min: 160, max: 760 },
  launchedBy: { defaultWidth: 120, min: 72, max: 280 },
  cpu: { defaultWidth: 64, min: 48, max: 160 },
  memory: { defaultWidth: 96, min: 64, max: 200 },
  pid: { defaultWidth: 72, min: 52, max: 140 },
  kind: { defaultWidth: 80, min: 56, max: 160 },
} as const satisfies Record<string, ColumnSpec>

export function clampWidth(spec: ColumnSpec, width: number): number {
  if (!Number.isFinite(width)) return spec.defaultWidth
  return Math.round(Math.min(spec.max, Math.max(spec.min, width)))
}

export function defaultWidths<K extends string>(specs: Record<K, ColumnSpec>): ColumnWidths<K> {
  return Object.fromEntries(
    Object.entries<ColumnSpec>(specs).map(([key, spec]) => [key, spec.defaultWidth]),
  ) as ColumnWidths<K>
}

/** Defaults overlaid with whatever valid widths `stored` holds; anything else is ignored. */
export function resolveWidths<K extends string>(specs: Record<K, ColumnSpec>, stored: unknown): ColumnWidths<K> {
  const widths = defaultWidths(specs)
  if (stored === null || typeof stored !== 'object') return widths
  for (const key of Object.keys(specs) as K[]) {
    const value = (stored as Record<string, unknown>)[key]
    if (typeof value === 'number') widths[key] = clampWidth(specs[key], value)
  }
  return widths
}

export function loadWidths<K extends string>(storageKey: string, specs: Record<K, ColumnSpec>): ColumnWidths<K> {
  try {
    return resolveWidths(specs, JSON.parse(globalThis.localStorage?.getItem(storageKey) ?? 'null'))
  } catch {
    return defaultWidths(specs)
  }
}

export function saveWidths(storageKey: string, widths: Record<string, number>): void {
  try {
    globalThis.localStorage?.setItem(storageKey, JSON.stringify(widths))
  } catch {
    // Widths are a convenience; losing them is harmless.
  }
}
