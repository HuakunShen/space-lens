export const SERVICE_NAME = 'space-lens-serve'

/**
 * Bump API_MAJOR whenever the contract changes in a way that would make an
 * older UI drive a newer host incorrectly. A mismatch means the two sides do
 * not share a contract and the session must be discarded rather than used.
 */
export const API_MAJOR = 1

export const CONTRACT_VERSION = '1.0.0'
