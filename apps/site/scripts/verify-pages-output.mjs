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

// Regression: the landing page once used a hand-drawn CSS symbol instead of
// the Space Lens app icon in both navigation locations.
const brandMarks = html.match(/<img\b[^>]*class="brand-mark"[^>]*src="(?:\.\/|\/)logo-mark\.png"[^>]*>/g) ?? []
assert.equal(brandMarks.length, 2, 'Header and footer must use the real Space Lens logo')

// Regression: the browser tab had a separate, invented favicon.
assert.match(html, /<link rel="icon" href="(?:\.\/|\/)favicon\.png" type="image\/png"\s*\/?\s*>/)
for (const asset of ['logo-mark.png', 'favicon.png']) {
  assert.deepEqual(
    readFileSync(join(output, asset)),
    readFileSync(join(siteRoot, '../web/static', asset)),
    `${asset} must match the production web app asset byte for byte`,
  )
}

console.log('Custom-domain root paths, authentic app branding, and Pages artifact verified.')
