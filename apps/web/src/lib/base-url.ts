/**
 * Where the app connects, decided at runtime.
 *
 * The bundle is static: it is built once and served by whatever service happens to be
 * running, so the service address cannot be baked in at build time. It is resolved here,
 * in a pure function, so the rules are testable without a browser:
 *
 * 1. an explicit `?api=` in the URL (a service on another port, or a tunnel);
 * 2. a remembered address from a previous visit in this browser;
 * 3. otherwise this page's own origin — the normal case, because the service serves the
 *    page it is protecting, which is also why there is no CORS in the default path.
 *
 * A root-relative `?api=` (e.g. `/space-lens/d/<dir>` from the Harness plugin's pairing
 * redirect) deliberately retains the renderer's transport — web stays on HTTP, desktop
 * stays on `dsh-app://app` — so the embedded frame never escapes to a foreign origin
 * and never violates `connect-src 'self'`.
 */
export interface BaseUrlInput {
  readonly queryApi: string | null
  readonly storedBaseUrl: string | null
  readonly pageOrigin: string
}

export function parseBaseUrl(input: BaseUrlInput): { url: string; sameOrigin: boolean } {
  const normalized = normalizeBaseUrl(input.queryApi ?? input.storedBaseUrl) ?? input.pageOrigin
  const url = normalized.replace(/\/$/, '')
  const sameOrigin = url === input.pageOrigin || (url.startsWith('/') && !url.startsWith('//'))
  return { url, sameOrigin }
}

/**
 * A normalised HTTP(S) address or root-relative embedded mount, or null when unusable.
 * A relative mount deliberately retains the renderer's transport (including an embedder's
 * secure custom scheme); it must never become an HTTP override or a network-path URL.
 *
 * The path is part of the address, not noise. A service mounted under a prefix — an
 * embedding host that serves the workbench at `/space-lens` beside its own routes — is a real
 * deployment, and dropping the path there would send every request to the host's own
 * `/api/v1/…`. The query and fragment stay meaningless, and trailing slashes are removed
 * because every request is built as `${baseUrl}${path}`: keeping one would request
 * `//api/v1/…`.
 */
export function normalizeBaseUrl(value: string | null | undefined): string | null {
  if (value !== null && value !== undefined && /[\u0000-\u001f\u007f]/.test(value)) {
    return null
  }
  const trimmed = value?.trim() ?? ''
  if (trimmed === '') {
    return null
  }
  if (trimmed.startsWith('/')) {
    if (trimmed.startsWith('//') || trimmed.includes('\\')) return null
    const path = trimmed.split(/[?#]/, 1)[0] ?? ''
    // URL parsers erase dot segments before fetching, including percent-encoded dots.
    if (path.split('/').some((segment) => /^\.{1,2}$/.test(segment.replace(/%2e/gi, '.')))) return null
    return path.replace(/\/+$/, '')
  }
  let url: URL | null = null
  try {
    url = new URL(trimmed)
  } catch {
    return null
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    return null
  }
  return `${url.origin}${url.pathname.replace(/\/+$/, '')}`
}
