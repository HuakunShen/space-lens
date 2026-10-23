/**
 * Minimal, safe markdown for the advisor transcript. Model output is untrusted:
 * every character is HTML-escaped first and only a fixed set of tags is ever
 * emitted, so there is no path from model text to markup, attributes or URLs
 * (links are not supported on purpose).
 */

function escapeHtml(text: string): string {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
}

function inline(raw: string): string {
  // Code spans become placeholders first, so bold may wrap a code span (the
  // model writes "**`/Users/me` = 160 GB**") and code is never re-formatted.
  // NUL never survives from the model text, so a placeholder cannot be forged.
  const code: string[] = []
  const text = escapeHtml(raw.replace(/\u0000/g, '')).replace(/`([^`]+)`/g, (_match, body: string) => {
    code.push(body)
    return `\u0000${code.length - 1}\u0000`
  })
  return text
    .replace(/\*\*(?!\s)(.+?)(?<!\s)\*\*/g, '<strong>$1</strong>')
    .replace(/(^|[^*])\*([^*\s][^*]*)\*(?!\*)/g, '$1<em>$2</em>')
    .replace(/\u0000(\d+)\u0000/g, (_match, index: string) => `<code>${code[Number(index)] ?? ''}</code>`)
}

function cells(line: string): string[] {
  return line
    .trim()
    .replace(/^\|/, '')
    .replace(/\|$/, '')
    .split('|')
    .map((cell) => cell.trim())
}

const isTableRule = (line: string): boolean => /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/.test(line)

export function renderMarkdown(source: string): string {
  const lines = source.replace(/\r\n/g, '\n').split('\n')
  const out: string[] = []
  let index = 0
  while (index < lines.length) {
    const line = lines[index] ?? ''
    if (line.trim() === '') {
      index++
      continue
    }
    if (line.trim().startsWith('```')) {
      const code: string[] = []
      index++
      while (index < lines.length && !(lines[index] ?? '').trim().startsWith('```')) code.push(lines[index++] ?? '')
      index++
      out.push(`<pre><code>${escapeHtml(code.join('\n'))}</code></pre>`)
      continue
    }
    const heading = /^(#{1,4})\s+(.*)$/.exec(line)
    if (heading) {
      const level = Math.min(4, heading[1]!.length + 2)
      out.push(`<h${level}>${inline(heading[2] ?? '')}</h${level}>`)
      index++
      continue
    }
    if (line.includes('|') && isTableRule(lines[index + 1] ?? '')) {
      const head = cells(line)
      index += 2
      const rows: string[][] = []
      while (index < lines.length && (lines[index] ?? '').includes('|') && (lines[index] ?? '').trim() !== '') rows.push(cells(lines[index++] ?? ''))
      out.push(
        `<table><thead><tr>${head.map((c) => `<th>${inline(c)}</th>`).join('')}</tr></thead><tbody>${rows
          .map((row) => `<tr>${row.map((c) => `<td>${inline(c)}</td>`).join('')}</tr>`)
          .join('')}</tbody></table>`,
      )
      continue
    }
    const bullet = /^\s*[-*]\s+/
    const numbered = /^\s*\d+[.)]\s+/
    if (bullet.test(line) || numbered.test(line)) {
      const ordered = numbered.test(line)
      const matcher = ordered ? numbered : bullet
      const items: string[] = []
      while (index < lines.length && matcher.test(lines[index] ?? '')) items.push((lines[index++] ?? '').replace(matcher, ''))
      const tag = ordered ? 'ol' : 'ul'
      out.push(`<${tag}>${items.map((item) => `<li>${inline(item)}</li>`).join('')}</${tag}>`)
      continue
    }
    const paragraph: string[] = []
    while (index < lines.length && (lines[index] ?? '').trim() !== '' && !/^(#{1,4}\s|\s*[-*]\s|\s*\d+[.)]\s|```)/.test(lines[index] ?? '')) {
      if ((lines[index] ?? '').includes('|') && isTableRule(lines[index + 1] ?? '')) break
      paragraph.push(lines[index++] ?? '')
    }
    if (paragraph.length === 0) {
      paragraph.push(lines[index++] ?? '')
    }
    out.push(`<p>${paragraph.map(inline).join('<br>')}</p>`)
  }
  return out.join('')
}
