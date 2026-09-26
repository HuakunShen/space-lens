import { parseCanonicalDeviceIdV1, type DeviceIdV1 } from "../contracts/view-v1/ids";

/** Which independently versioned UI pack owns this context. */
export type ViewPackIdV1 = "refyard" | "space-lens";

export type ViewLocaleV1 = "en" | "zh-Hans" | "ja" | "es" | "fr";

/**
 * The immutable identity captured when a native-backed view session opens.
 * Byte arrays are represented as lowercase hex so fixtures remain canonical
 * JSON and do not depend on a transport-specific binary encoding.
 */
export interface ViewContextV1 {
  readonly contractMajor: number;
  /** Sorted, unique feature names negotiated by this exact contract major. */
  readonly featureBits: readonly string[];
  readonly packId: ViewPackIdV1;
  /** SHA-256 digest of the verified pack, encoded as 64 lowercase hex chars. */
  readonly packDigest: string;
  readonly xrossVersion: string;
  /** Source-client incarnation, encoded as 32 lowercase hex chars. */
  readonly sourceClientIncarnation: string;
  readonly targetDeviceId: DeviceIdV1;
  readonly targetDisplayLabel: string;
  /** Canonical base-10 u64 text, preserving all bits in JSON/JavaScript. */
  readonly targetPolicyRevision: string;
  /** Fresh bridge generation, encoded as 32 lowercase hex chars. */
  readonly bridgeGeneration: string;
  readonly locale: ViewLocaleV1;
}

const VIEW_CONTEXT_FIELDS = [
  "contractMajor",
  "featureBits",
  "packId",
  "packDigest",
  "xrossVersion",
  "sourceClientIncarnation",
  "targetDeviceId",
  "targetDisplayLabel",
  "targetPolicyRevision",
  "bridgeGeneration",
  "locale",
] as const;

const HEX_16_BYTES = /^[0-9a-f]{32}$/;
const HEX_SHA256 = /^[0-9a-f]{64}$/;
const SEMVER = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;
const CANONICAL_U64 = /^(0|[1-9][0-9]{0,19})$/;
const MAX_U64_DECIMAL = "18446744073709551615";
const VIEW_LOCALES: ReadonlySet<string> = new Set(["en", "zh-Hans", "ja", "es", "fr"]);

function isRecord(value: unknown): value is Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

function hasValidUnicodeScalars(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const codeUnit = value.charCodeAt(index);
    if (codeUnit >= 0xd800 && codeUnit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (!(next >= 0xdc00 && next <= 0xdfff)) return false;
      index += 1;
    } else if (codeUnit >= 0xdc00 && codeUnit <= 0xdfff) {
      return false;
    }
  }
  return true;
}

function isBoundedDisplayLabel(value: string): boolean {
  return (
    value.length > 0 &&
    hasValidUnicodeScalars(value) &&
    new TextEncoder().encode(value).byteLength <= 128
  );
}

function isCanonicalU64(value: string): boolean {
  if (!CANONICAL_U64.test(value)) return false;
  return value.length < MAX_U64_DECIMAL.length || value <= MAX_U64_DECIMAL;
}

/** Parses and copies an untrusted native handshake into a strict immutable DTO. */
export function parseViewContextV1(value: unknown): ViewContextV1 | null {
  try {
    if (!isRecord(value)) return null;
    const ownKeys = Reflect.ownKeys(value);
    if (
      ownKeys.length !== VIEW_CONTEXT_FIELDS.length ||
      ownKeys.some((key) => typeof key !== "string" || !VIEW_CONTEXT_FIELDS.includes(key as (typeof VIEW_CONTEXT_FIELDS)[number]))
    ) {
      return null;
    }

    const {
      contractMajor,
      featureBits,
      packId,
      packDigest,
      xrossVersion,
      sourceClientIncarnation,
      targetDeviceId,
      targetDisplayLabel,
      targetPolicyRevision,
      bridgeGeneration,
      locale,
    } = value;

    if (typeof contractMajor !== "number" || !Number.isInteger(contractMajor) || contractMajor < 0 || contractMajor > 65535) return null;
    if (
      !Array.isArray(featureBits) ||
      !featureBits.every(
        (feature) =>
          typeof feature === "string" &&
          feature.length > 0 &&
          hasValidUnicodeScalars(feature) &&
          feature.normalize("NFC") === feature,
      )
    ) return null;
    const featureNames = featureBits as string[];
    for (let index = 1; index < featureNames.length; index += 1) {
      const previous = featureNames[index - 1];
      const current = featureNames[index];
      if (previous === undefined || current === undefined || previous >= current) return null;
    }
    if (packId !== "refyard" && packId !== "space-lens") return null;
    if (typeof packDigest !== "string" || !HEX_SHA256.test(packDigest)) return null;
    if (typeof xrossVersion !== "string" || xrossVersion.length > 32 || !SEMVER.test(xrossVersion)) return null;
    if (typeof sourceClientIncarnation !== "string" || !HEX_16_BYTES.test(sourceClientIncarnation)) return null;
    if (typeof targetDeviceId !== "string") return null;
    const canonicalTargetDeviceId = parseCanonicalDeviceIdV1(targetDeviceId);
    if (canonicalTargetDeviceId === null) return null;
    if (typeof targetDisplayLabel !== "string" || !isBoundedDisplayLabel(targetDisplayLabel)) return null;
    if (typeof targetPolicyRevision !== "string" || !isCanonicalU64(targetPolicyRevision)) return null;
    if (typeof bridgeGeneration !== "string" || !HEX_16_BYTES.test(bridgeGeneration)) return null;
    if (typeof locale !== "string" || !VIEW_LOCALES.has(locale)) return null;

    return Object.freeze({
      contractMajor,
      featureBits: Object.freeze([...featureNames]),
      packId,
      packDigest,
      xrossVersion,
      sourceClientIncarnation,
      targetDeviceId: canonicalTargetDeviceId,
      targetDisplayLabel,
      targetPolicyRevision,
      bridgeGeneration,
      locale: locale as ViewLocaleV1,
    });
  } catch {
    return null;
  }
}

/**
 * Prevents results captured by an earlier pack, target, policy, app/client
 * incarnation, or bridge generation from being applied to the current view.
 * Comparing the full handshake also fails closed if a host accidentally
 * reuses a generation while changing one of its associated identities.
 */
export function isCurrentViewContext(
  captured: ViewContextV1,
  current: ViewContextV1,
): boolean {
  return (
    captured.contractMajor === current.contractMajor &&
    captured.featureBits.length === current.featureBits.length &&
    captured.featureBits.every((feature, index) => feature === current.featureBits[index]) &&
    captured.packId === current.packId &&
    captured.packDigest === current.packDigest &&
    captured.xrossVersion === current.xrossVersion &&
    captured.sourceClientIncarnation === current.sourceClientIncarnation &&
    captured.targetDeviceId === current.targetDeviceId &&
    captured.targetDisplayLabel === current.targetDisplayLabel &&
    captured.targetPolicyRevision === current.targetPolicyRevision &&
    captured.bridgeGeneration === current.bridgeGeneration &&
    captured.locale === current.locale
  );
}
