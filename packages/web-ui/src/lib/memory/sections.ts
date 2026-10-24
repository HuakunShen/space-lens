/**
 * The Memory workbench's four sections. The sidebar is a real section switcher,
 * not a copy of Storage's locations: Overview answers "how is memory doing?",
 * Processes answers "who is using it?", Stale surfaces likely leftovers, and
 * Activity answers "what did I quit?". The literal list is written once and
 * shared by the sidebar, the page and the page title, so a section can never be
 * added to one and missed by another.
 */
export const MEMORY_SECTIONS = ['overview', 'processes', 'stale', 'activity'] as const

export type MemorySection = (typeof MEMORY_SECTIONS)[number]

export const MEMORY_SECTION_LABELS: Record<MemorySection, string> = {
  overview: 'Overview',
  processes: 'Processes',
  stale: 'Stale',
  activity: 'Activity',
}

export function parseMemorySection(value: string | null): MemorySection {
  return MEMORY_SECTIONS.find((section) => section === value) ?? 'overview'
}
