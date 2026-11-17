import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import postcss from 'postcss'

// Inspect the CSS emitted by the production build, including the shared app
// stylesheet, at a desktop viewport. The app shell legitimately disables
// scrolling, but the marketing page must not inherit that behavior.
const buildRoot = join(dirname(fileURLToPath(import.meta.url)), '..', 'build')
const html = readFileSync(join(buildRoot, 'index.html'), 'utf8')
const assets = [...html.matchAll(/href="\.\/_app\/immutable\/assets\/([^"/]+\.css)"/g)]
assert.ok(assets.length > 0, 'Expected built CSS in the prerendered page')

const computed = {
  html: { overflowY: 'visible', height: 'auto' },
  body: { overflowY: 'visible', height: 'auto' },
}

function appliesOnDesktop(rule) {
  for (let parent = rule.parent; parent; parent = parent.parent) {
    if (parent.type !== 'atrule' || parent.name !== 'media') continue
    const media = parent.params.replaceAll(' ', '')
    if (/max-width:\d+px|width<=\d+px/.test(media)) return false
  }
  return true
}

for (const [, filename] of assets) {
  const css = postcss.parse(readFileSync(join(buildRoot, '_app/immutable/assets', filename), 'utf8'))
  css.walkRules((rule) => {
    if (!appliesOnDesktop(rule)) return
    for (const element of ['html', 'body']) {
      if (!rule.selectors.some((selector) => selector.trim() === element)) continue
      rule.walkDecls((declaration) => {
        if (declaration.prop === 'overflow' || declaration.prop === 'overflow-y') {
          computed[element].overflowY = declaration.value.split(' ')[0]
        }
        if (declaration.prop === 'height') computed[element].height = declaration.value
      })
    }
  })
}

for (const [element, style] of Object.entries(computed)) {
  assert.notEqual(style.overflowY, 'hidden', `${element} must not hide the landing page scroll: ${JSON.stringify(computed)}`)
  assert.notEqual(style.height, '100%', `${element} must size to its landing page content: ${JSON.stringify(computed)}`)
}
console.log(`Landing page desktop scroll CSS verified: ${JSON.stringify(computed)}`)
