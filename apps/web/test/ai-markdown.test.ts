import assert from 'node:assert/strict'
import { test } from 'node:test'
import { renderMarkdown } from '../src/lib/ai/markdown.ts'

test('escapes html and never emits attributes or links', () => {
  const html = renderMarkdown('<img src=x onerror=alert(1)> [a](javascript:alert(1)) <script>x</script>')
  assert.ok(!html.includes('<img') && !html.includes('<script'))
  assert.ok(!html.includes('<a '))
  assert.ok(html.includes('&lt;img'))
})

test('renders bold, code, lists and headings', () => {
  assert.equal(renderMarkdown('Here **30 MB** in `node_modules`'), '<p>Here <strong>30 MB</strong> in <code>node_modules</code></p>')
  assert.equal(renderMarkdown('- a\n- b'), '<ul><li>a</li><li>b</li></ul>')
  assert.equal(renderMarkdown('1. a\n2. b'), '<ol><li>a</li><li>b</li></ol>')
  assert.equal(renderMarkdown('## Title'), '<h4>Title</h4>')
})

test('renders the table the model produced, escaping cells', () => {
  const html = renderMarkdown('| Candidate | Risk |\n|---|---|\n| `.cache` | **confirm** <b> |')
  assert.ok(html.includes('<th>Candidate</th>') && html.includes('<td><code>.cache</code></td>'))
  assert.ok(html.includes('<strong>confirm</strong> &lt;b&gt;'))
})

test('code fences stay literal', () => {
  assert.equal(renderMarkdown('```\n**x** <b>\n```'), '<pre><code>**x** &lt;b&gt;</code></pre>')
})
