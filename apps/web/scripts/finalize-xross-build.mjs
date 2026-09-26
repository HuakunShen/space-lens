/** Keep the native view's default document on /xross and omit service-worker code. */
import { copyFileSync, existsSync, readFileSync, unlinkSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const flavor = process.argv[2]
if (flavor !== 'local' && flavor !== 'hosted') throw new Error('expected local or hosted')
const output = join(import.meta.dirname, '..', `build-xross-${flavor}`)
const entry = join(output, 'xross-pack.html')
if (!existsSync(entry)) throw new Error(`missing Xross route entry: ${entry}`)
writeFileSync(join(output, 'index.html'), readFileSync(entry))
copyFileSync(join(import.meta.dirname, '..', 'static', 'favicon.png'), join(output, 'favicon.png'))
const worker = join(output, 'service-worker.js')
if (existsSync(worker)) unlinkSync(worker)
