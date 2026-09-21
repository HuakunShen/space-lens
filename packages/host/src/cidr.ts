import { BlockList, isIP } from 'node:net'

export class CidrError extends Error {}

function parseCidr(cidr: string): { address: string; prefix: number; family: 'ipv4' | 'ipv6' } {
  const separator = cidr.lastIndexOf('/')
  const address = separator === -1 ? cidr : cidr.slice(0, separator)
  const prefixText = separator === -1 ? undefined : cidr.slice(separator + 1)
  const family = isIP(address)
  if (family === 0) throw new CidrError(`invalid CIDR address: ${cidr}`)
  const width = family === 4 ? 32 : 128
  const prefix = prefixText === undefined ? width : Number.parseInt(prefixText, 10)
  if (!Number.isInteger(prefix) || prefix < 0 || prefix > width) throw new CidrError(`invalid CIDR prefix: ${cidr}`)
  return { address, prefix, family: family === 4 ? 'ipv4' : 'ipv6' }
}

/** Map IPv4-mapped IPv6 remote addresses (`::ffff:192.168.1.5`) back to IPv4. */
export function normalizeRemoteAddress(address: string): string {
  if (address.startsWith('::ffff:')) {
    const mapped = address.slice('::ffff:'.length)
    if (isIP(mapped) === 4) return mapped
  }
  return address
}

export interface ClientAllowlist {
  check(remoteAddress: string): boolean
}

/**
 * Client-address gate. Loopback is always permitted so a local admin can never
 * lock themselves out of a server they started; configured CIDRs widen it.
 *
 * Node's BlockList answers family-mixed queries unreliably once both families
 * are present, so v4 and v6 subnets live in separate lists and the (already
 * normalized) address picks the list.
 */
export function createClientAllowlist(cidrs: readonly string[]): ClientAllowlist {
  const v4 = new BlockList()
  const v6 = new BlockList()
  const add = (cidr: string): void => {
    const parsed = parseCidr(cidr)
    const list = parsed.family === 'ipv4' ? v4 : v6
    list.addSubnet(parsed.address, parsed.prefix, parsed.family)
  }
  for (const cidr of ['127.0.0.0/8', '::1/128', ...cidrs]) add(cidr)
  return {
    check(remoteAddress: string): boolean {
      const normalized = normalizeRemoteAddress(remoteAddress)
      const family = isIP(normalized)
      try {
        // Node's BlockList infers families unreliably for IPv6 unless the
        // type is stated explicitly
        if (family === 4) return v4.check(normalized, 'ipv4')
        if (family === 6) return v6.check(normalized, 'ipv6')
        return false
      } catch {
        return false
      }
    },
  }
}

export function validateCidrs(cidrs: readonly string[]): void {
  for (const cidr of cidrs) parseCidr(cidr)
}
