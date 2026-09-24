import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { buildContractArtifacts } from '../src/artifacts.ts'

const here = dirname(fileURLToPath(import.meta.url))
const outDir = join(here, '..', 'generated')
mkdirSync(outDir, { recursive: true })

for (const [name, content] of Object.entries(buildContractArtifacts())) {
  writeFileSync(join(outDir, name), content)
  console.log(`wrote generated/${name}`)
}
