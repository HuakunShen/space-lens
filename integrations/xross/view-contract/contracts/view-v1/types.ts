import type { CursorIdV1 } from "./ids";

/** Canonical, JSON-safe primitives shared by the two local view facades. */
export type UInt64V1 = string & { readonly __uint64V1: unique symbol };
export type UnixMillisV1 = UInt64V1;
export type CursorV1 = CursorIdV1;

export interface PageV1<T> {
  readonly items: readonly T[];
  readonly nextCursor: CursorV1 | null;
}

export type RemoteEndpointSummaryV1 =
  | { readonly kind: "network"; readonly transport: "https" | "http" | "ssh" | "git"; readonly hostDisplay: string }
  | { readonly kind: "redacted" };

export type ViewErrorDomainV1 = "host" | "refyard" | "space-lens";

export type HostViewErrorCodeV1 =
  | "HostUnavailable"
  | "IncompatibleContract"
  | "PermissionDenied"
  | "TargetOffline"
  | "ResourceUnavailable"
  | "RecoveryUnavailable"
  | "UnsupportedOnTarget"
  | "MalformedPayload"
  | "LimitExceeded"
  | "StaleGeneration"
  | "StreamGap"
  | "Internal";

export type RefyardViewErrorCodeV1 =
  | "Unauthenticated"
  | "Forbidden"
  | "NotFound"
  | "InvalidRequest"
  | "UnsupportedOperation"
  | "InvalidOperationPayload"
  | "UnsupportedPathEncoding"
  | "StaleSnapshot"
  | "StalePreview"
  | "Conflict"
  | "IdempotencyConflict"
  | "ResourceBusy"
  | "LimitExceeded"
  | "GitCommandFailed"
  | "NeedsAttention"
  | "UncertainOutcome"
  | "Timeout"
  | "Cancelled"
  | "Unavailable"
  | "ProviderNotConnected"
  | "NoProviderRemote"
  | "ProviderUnauthorized"
  | "ProviderRateLimited"
  | "InternalError";

export type SpaceLensViewErrorCodeV1 =
  | "Unauthenticated"
  | "Forbidden"
  | "NotFound"
  | "InvalidRequest"
  | "UnsupportedOperation"
  | "LimitExceeded"
  | "StalePlan"
  | "StaleSnapshot"
  | "Conflict"
  | "Timeout"
  | "Cancelled"
  | "Unavailable"
  | "InternalError";

export type ViewErrorParameterV1 =
  | { readonly kind: "fieldName"; readonly fieldName: ViewFieldNameV1 }
  | {
      readonly kind: "limit";
      readonly limitName: ViewLimitNameV1;
      readonly maximum: number;
      readonly actual: number;
    }
  | { readonly kind: "retryAfter"; readonly retryAfterMs: number };

export type ViewFieldNameV1 =
  | "cursor"
  | "limit"
  | "rootIds"
  | "pathIds"
  | "nodeIds"
  | "relativeDepth"
  | "maxChildren"
  | "maxNodes"
  | "operation"
  | "target"
  | "snapshotId"
  | "worktreeId";

export type ViewLimitNameV1 =
  | "requestBytes"
  | "responseBytes"
  | "pageItems"
  | "cursorBytes"
  | "displayBytes"
  | "diffBytes"
  | "streamEventBytes"
  | "rootIds"
  | "pathIds"
  | "nodeIds"
  | "relativeDepth"
  | "maxChildren"
  | "maxNodes"
  | "ancestors";

export interface ViewErrorV1 {
  readonly domain: ViewErrorDomainV1;
  readonly code: HostViewErrorCodeV1 | RefyardViewErrorCodeV1 | SpaceLensViewErrorCodeV1;
  readonly retryable: boolean;
  readonly parameters: readonly ViewErrorParameterV1[];
}

export type SequencedViewEventV1<T> =
  | { readonly kind: "event"; readonly sequence: UInt64V1; readonly value: T }
  | {
      readonly kind: "gap";
      readonly requestedAfter: UInt64V1;
      readonly oldestAvailable: UInt64V1;
    }
  | {
      readonly kind: "terminal";
      readonly sequence: UInt64V1;
      readonly outcome: "completed" | "cancelled" | "failed";
    };

export interface RuntimeLimitsV1 {
  readonly historyDefaultPageSize: number;
  readonly historyMaxPageSize: number;
  readonly patchMaxBytesPerFile: number;
  readonly patchMaxLinesPerFile: number;
  readonly objectMaxBytes: number;
  readonly logicalCacheMaxBytes: number;
  readonly previewTokenTtlSeconds: number;
  readonly readonlyDeadlineSeconds: number;
  readonly networkDeadlineSeconds: number;
  readonly hookDeadlineSeconds: number;
  readonly queuedOperationsPerActor: number;
  readonly concurrentGitProcesses: number;
  readonly concurrentReadersPerRepository: number;
  readonly eventRingMaxEvents: number;
  readonly eventRingMaxBytes: number;
  readonly pathSelectionMaxEntries: number;
  readonly historyTipsMax: number;
  readonly commitMessageMaxBytes: number;
  readonly branchNameMaxLength: number;
}

export type ApprovalResultV1<T> =
  | { readonly decision: "approved"; readonly outcome: T }
  | { readonly decision: "denied" }
  | { readonly decision: "cancelled" };

const MAX_UINT64_V1 = "18446744073709551615";
const CANONICAL_UINT64_V1 = /^(0|[1-9][0-9]{0,19})$/;

/** Parses the decimal JSON representation of an unsigned 64-bit value exactly. */
export function parseUInt64V1(value: unknown): UInt64V1 | null {
  if (typeof value !== "string" || !CANONICAL_UINT64_V1.test(value)) return null;
  if (value.length === MAX_UINT64_V1.length && value > MAX_UINT64_V1) return null;
  return value as UInt64V1;
}

/** Unix milliseconds use the same nonnegative, exact wire representation. */
export function parseUnixMillisV1(value: unknown): UnixMillisV1 | null {
  return parseUInt64V1(value);
}

const HOST_ERROR_CODES_V1: readonly HostViewErrorCodeV1[] = [
  "HostUnavailable",
  "IncompatibleContract",
  "PermissionDenied",
  "TargetOffline",
  "ResourceUnavailable",
  "RecoveryUnavailable",
  "UnsupportedOnTarget",
  "MalformedPayload",
  "LimitExceeded",
  "StaleGeneration",
  "StreamGap",
  "Internal",
];

export const REFYARD_VIEW_ERROR_CODES_V1: readonly RefyardViewErrorCodeV1[] = [
  "Unauthenticated",
  "Forbidden",
  "NotFound",
  "InvalidRequest",
  "UnsupportedOperation",
  "InvalidOperationPayload",
  "UnsupportedPathEncoding",
  "StaleSnapshot",
  "StalePreview",
  "Conflict",
  "IdempotencyConflict",
  "ResourceBusy",
  "LimitExceeded",
  "GitCommandFailed",
  "NeedsAttention",
  "UncertainOutcome",
  "Timeout",
  "Cancelled",
  "Unavailable",
  "ProviderNotConnected",
  "NoProviderRemote",
  "ProviderUnauthorized",
  "ProviderRateLimited",
  "InternalError",
];

export const SPACE_LENS_VIEW_ERROR_CODES_V1: readonly SpaceLensViewErrorCodeV1[] = [
  "Unauthenticated",
  "Forbidden",
  "NotFound",
  "InvalidRequest",
  "UnsupportedOperation",
  "LimitExceeded",
  "StalePlan",
  "StaleSnapshot",
  "Conflict",
  "Timeout",
  "Cancelled",
  "Unavailable",
  "InternalError",
];

const FIELD_NAMES_V1: readonly ViewFieldNameV1[] = [
  "cursor",
  "limit",
  "rootIds",
  "pathIds",
  "nodeIds",
  "relativeDepth",
  "maxChildren",
  "maxNodes",
  "operation",
  "target",
  "snapshotId",
  "worktreeId",
];

const LIMIT_NAMES_V1: readonly ViewLimitNameV1[] = [
  "requestBytes",
  "responseBytes",
  "pageItems",
  "cursorBytes",
  "displayBytes",
  "diffBytes",
  "streamEventBytes",
  "rootIds",
  "pathIds",
  "nodeIds",
  "relativeDepth",
  "maxChildren",
  "maxNodes",
  "ancestors",
];

function isPlainRecordV1(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function hasExactKeysV1(value: Record<string, unknown>, keys: readonly string[]): boolean {
  const actual = Object.keys(value);
  return actual.length === keys.length && keys.every((key) => Object.hasOwn(value, key));
}

function isSafeNonnegativeIntegerV1(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

/**
 * Validates the closed, non-sensitive error DTO before a surface displays it.
 * Raw host/service messages are deliberately not part of this wire shape.
 */
export function parseViewErrorV1(value: unknown): ViewErrorV1 | null {
  if (!isPlainRecordV1(value) || !hasExactKeysV1(value, ["domain", "code", "retryable", "parameters"])) {
    return null;
  }

  const domain = value.domain;
  const code = value.code;
  const validCode =
    (domain === "host" && typeof code === "string" && HOST_ERROR_CODES_V1.includes(code as HostViewErrorCodeV1)) ||
    (domain === "refyard" && typeof code === "string" && REFYARD_VIEW_ERROR_CODES_V1.includes(code as RefyardViewErrorCodeV1)) ||
    (domain === "space-lens" && typeof code === "string" && SPACE_LENS_VIEW_ERROR_CODES_V1.includes(code as SpaceLensViewErrorCodeV1));
  if (!validCode || typeof value.retryable !== "boolean" || !Array.isArray(value.parameters)) return null;

  const parameters: ViewErrorParameterV1[] = [];
  for (const parameter of value.parameters) {
    if (!isPlainRecordV1(parameter) || typeof parameter.kind !== "string") return null;
    if (parameter.kind === "fieldName") {
      if (
        !hasExactKeysV1(parameter, ["kind", "fieldName"]) ||
        typeof parameter.fieldName !== "string" ||
        !FIELD_NAMES_V1.includes(parameter.fieldName as ViewFieldNameV1)
      ) return null;
      parameters.push({ kind: "fieldName", fieldName: parameter.fieldName as ViewFieldNameV1 });
      continue;
    }
    if (parameter.kind === "limit") {
      if (
        !hasExactKeysV1(parameter, ["kind", "limitName", "maximum", "actual"]) ||
        typeof parameter.limitName !== "string" ||
        !LIMIT_NAMES_V1.includes(parameter.limitName as ViewLimitNameV1) ||
        !isSafeNonnegativeIntegerV1(parameter.maximum) ||
        !isSafeNonnegativeIntegerV1(parameter.actual)
      ) return null;
      parameters.push({
        kind: "limit",
        limitName: parameter.limitName as ViewLimitNameV1,
        maximum: parameter.maximum,
        actual: parameter.actual,
      });
      continue;
    }
    if (parameter.kind === "retryAfter") {
      if (!hasExactKeysV1(parameter, ["kind", "retryAfterMs"]) || !isSafeNonnegativeIntegerV1(parameter.retryAfterMs)) {
        return null;
      }
      parameters.push({ kind: "retryAfter", retryAfterMs: parameter.retryAfterMs });
      continue;
    }
    return null;
  }

  return {
    domain,
    code: code as ViewErrorV1["code"],
    retryable: value.retryable,
    parameters,
  };
}
