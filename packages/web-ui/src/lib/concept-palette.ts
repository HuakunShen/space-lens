/** Pixel samples from 2026-10-09-chart-explorations/{01,02,03,04}-*.png. */
export const CONCEPT_PALETTE = [
  {
    name: 'blue',
    treemap: ['#3f9af9', '#3088eb'],
    icicle: ['#1a87fd', '#1476f2'],
    bubbles: ['#56c1fd', '#2798fd', '#1289f9'],
    strips: ['#1a8df6', '#2c69a4'],
  },
  {
    name: 'purple',
    treemap: ['#8f59eb', '#784cda'],
    icicle: ['#a553f9', '#723cda'],
    bubbles: ['#d080fc', '#a958f5', '#b368f8'],
    strips: ['#a95ff6', '#46299e'],
  },
  {
    name: 'green',
    treemap: ['#5ab06b', '#49a865'],
    icicle: ['#50bf6b', '#3aa863'],
    bubbles: ['#63d894', '#4aca82', '#64d897'],
    strips: ['#61d16a', '#256d4a'],
  },
  {
    name: 'orange',
    treemap: ['#e18d30', '#d57f27'],
    icicle: ['#ee9633', '#d17328'],
    bubbles: ['#faa245', '#eb7823', '#ea7625'],
    strips: ['#f0a241', '#895b40'],
  },
  {
    name: 'pink',
    treemap: ['#e15375', '#cf4565'],
    icicle: ['#e15479', '#ce4366'],
    bubbles: ['#fa8acf', '#dd5fac', '#e267b2'],
    strips: ['#ee5f87', '#92365a'],
  },
  {
    name: 'cyan',
    treemap: ['#3392db', '#2780ca'],
    icicle: ['#3392db', '#2780ca'],
    bubbles: ['#48c2e6', '#2eb3de', '#1cb7e0'],
    strips: ['#18b3cb', '#19658b'],
  },
  {
    name: 'gray',
    treemap: ['#71767e', '#606670'],
    icicle: ['#707a86', '#5f6a77'],
    bubbles: ['#c0c5c7', '#a7acb0', '#868f96'],
    strips: ['#8f9aa5', '#47535e'],
  },
  {
    name: 'yellow',
    treemap: ['#f5e274', '#e6c537'],
    icicle: ['#f5e274', '#e6c537'],
    bubbles: ['#f5e274', '#e7c941', '#e6c537'],
    strips: ['#f5e274', '#e6c537'],
  },
] as const

export type ConceptFamily = (typeof CONCEPT_PALETTE)[number]

export function conceptFamily(source: string): ConceptFamily | undefined {
  return CONCEPT_PALETTE.find((family) => family.treemap[0] === source)
}

export function namedConceptFamily(name: string): ConceptFamily | undefined {
  const key = name.toLowerCase().replace(/[\s_-]+/g, '')
  const family =
    key === 'users'
      ? 'blue'
      : key === 'applications' || key === 'movies'
        ? 'purple'
        : key === 'system'
          ? 'green'
          : key === 'library' || key === 'developercaches' || key === 'caches'
            ? 'orange'
            : key === 'iclouddrive' || key === 'icloudlocalcopies'
              ? 'pink'
              : key === 'downloads'
                ? 'cyan'
                : key === 'photoslibrary'
                  ? 'yellow'
                  : key === 'other'
                    ? 'gray'
                    : undefined
  return CONCEPT_PALETTE.find((candidate) => candidate.name === family)
}
