import { chmodSync } from 'node:fs'
import { resolve } from 'node:path'

chmodSync(resolve(import.meta.dirname, '../dist/cli.mjs'), 0o755)
