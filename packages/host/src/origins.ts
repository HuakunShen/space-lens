export type OriginVerdict = 'ok' | 'refused-host' | 'refused-origin' | 'refused-cross-site'

/**
 * Authorities this listener answers for. Built from the bind configuration at
 * startup — never from a request — so a Host header naming some other machine
 * (DNS rebinding) is refused before anything else runs.
 */
export function authoritiesFor(address: string, port: number): string[] {
  const authorities = new Set<string>()
  if (address === '0.0.0.0' || address === '::') {
    authorities.add(`127.0.0.1:${port}`)
    authorities.add(`localhost:${port}`)
    authorities.add(address === '::' ? `[::1]:${port}` : `localhost:${port}`)
  } else if (address === '::1') {
    authorities.add(`[::1]:${port}`)
    authorities.add(`localhost:${port}`)
  } else {
    authorities.add(`${address}:${port}`)
    if (address === '127.0.0.1') authorities.add(`localhost:${port}`)
  }
  return [...authorities]
}

export function originsForAuthorities(authorities: readonly string[]): string[] {
  return authorities.map((authority) => `http://${authority}`)
}

export interface OriginCheckInput {
  hostHeader: string | null
  origin: string | null
  secFetchSite: string | null
  /** `Sec-Fetch-Mode`; `navigate` marks a top-level page load. */
  secFetchMode?: string | null
}

export interface OriginPolicy {
  authorities: readonly string[]
  allowedOrigins: readonly string[]
}

/**
 * One gate, three questions, in order: is the request even addressed to us
 * (Host), does a browser-presented Origin match the allowlist exactly, and is
 * an Origin-less browser fetch quietly declaring itself same-site?
 *
 * A top-level *navigation* is allowed through even when it is cross-site: the
 * response is a document the linking page cannot read, and refusing it means
 * opening the workbench from a chat message or a bookmark shows a JSON error
 * instead of the app. Subresource and API requests still need an Origin the
 * policy knows, so a hostile page cannot read anything.
 */
export function checkOrigin(input: OriginCheckInput, policy: OriginPolicy): OriginVerdict {
  if (input.hostHeader === null || !policy.authorities.includes(input.hostHeader)) return 'refused-host'
  if (input.origin !== null) {
    if (!policy.allowedOrigins.includes(input.origin)) return 'refused-origin'
  }
  if (
    input.secFetchSite !== null &&
    input.origin === null &&
    input.secFetchSite === 'cross-site' &&
    input.secFetchMode !== 'navigate'
  ) {
    return 'refused-cross-site'
  }
  return 'ok'
}
