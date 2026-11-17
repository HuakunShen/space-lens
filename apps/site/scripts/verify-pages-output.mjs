import assert from 'node:assert/strict'
import { readFileSync, existsSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const siteRoot = join(dirname(fileURLToPath(import.meta.url)), '..')
const output = join(siteRoot, 'build')
const html = readFileSync(join(output, 'index.html'), 'utf8')

assert.match(html, /<link rel="canonical" href="https:\/\/space-lens\.huakun\.tech\/"\s*\/>/)
assert.match(html, /(?:href|src)="\.\/_app\/immutable\//)
assert.doesNotMatch(html, /(?:href|src)="(?:\/space-lens\/|https:\/\/huakunshen\.github\.io\/space-lens\/)/)
assert.ok(existsSync(join(output, '.nojekyll')))

console.log('Custom-domain root paths, canonical URL, and Pages artifact verified.')
