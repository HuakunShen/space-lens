export const REFYARD_ID_PREFIXES = [
  "srvc",
  "root",
  "repo",
  "wt",
  "path",
  "snap",
  "pt",
  "op",
  "cur",
] as const;

export type RefyardIdPrefix = (typeof REFYARD_ID_PREFIXES)[number];

declare const refyardIdBrand: unique symbol;

/** A Refyard target id whose prefix remains part of its static type. */
export type RefyardIdV1<Prefix extends RefyardIdPrefix> = string & {
  readonly [refyardIdBrand]: Prefix;
};

export type WorkspaceRootIdV1 = RefyardIdV1<"root">;
export type ServiceInstanceIdV1 = RefyardIdV1<"srvc">;
export type RepositoryIdV1 = RefyardIdV1<"repo">;
export type WorktreeIdV1 = RefyardIdV1<"wt">;
export type PathIdV1 = RefyardIdV1<"path">;
/** Target-private Refyard source snapshot id; not used in the Xross view API. */
export type RefyardSourceSnapshotIdV1 = RefyardIdV1<"snap">;
/** Pinned Refyard's per-path precondition token; never a mutation preview handle. */
export type PathPreviewTokenV1 = RefyardIdV1<"pt">;
/** Pinned Refyard's submitted operation id; target-private, not a WebView job id. */
export type RefyardSourceOperationIdV1 = RefyardIdV1<"op">;
export type CursorIdV1 = RefyardIdV1<"cur">;

export const REFYARD_VIEW_ID_PREFIX = {
  mutationPreview: "rpreview",
  job: "rjob",
  snapshot: "rsnapshot",
  mutationRecovery: "rrecovery",
  mutationRecoverySnapshot: "rrecoverysnapshot",
} as const;

export type RefyardViewIdKindV1 = keyof typeof REFYARD_VIEW_ID_PREFIX;

declare const refyardViewIdBrand: unique symbol;

/** Xross-owned, domain-tagged Refyard view handles; distinct from source IDs. */
export type RefyardViewIdV1<Kind extends RefyardViewIdKindV1> = string & {
  readonly [refyardViewIdBrand]: Kind;
};

export type MutationPreviewIdV1 = RefyardViewIdV1<"mutationPreview">;
export type RefyardJobIdV1 = RefyardViewIdV1<"job">;
export type RefyardSnapshotIdV1 = RefyardViewIdV1<"snapshot">;
export type MutationRecoveryIdV1 = RefyardViewIdV1<"mutationRecovery">;
export type MutationRecoverySnapshotIdV1 = RefyardViewIdV1<"mutationRecoverySnapshot">;

const REFYARD_VIEW_ID_SUFFIX = /^[0-9a-f]{32}$/;

/** Parses Xross-owned 128-bit handles without accepting Refyard source IDs. */
export function parseRefyardViewId<Kind extends RefyardViewIdKindV1>(
  kind: Kind,
  value: string,
): RefyardViewIdV1<Kind> | null {
  const marker = `${REFYARD_VIEW_ID_PREFIX[kind]}_`;
  if (!value.startsWith(marker)) return null;

  const suffix = value.slice(marker.length);
  if (!REFYARD_VIEW_ID_SUFFIX.test(suffix)) return null;

  return value as RefyardViewIdV1<Kind>;
}

const REFYARD_SUFFIX = /^[A-Za-z0-9_-]{1,96}$/;

/**
 * Parses one exact Refyard ID domain. This validates syntax only: possession
 * never grants access, and a WorkspaceRootId is not a filesystem path.
 */
export function parseRefyardId<Prefix extends RefyardIdPrefix>(
  prefix: Prefix,
  value: string,
): RefyardIdV1<Prefix> | null {
  const marker = `${prefix}_`;
  if (!value.startsWith(marker)) return null;

  const suffix = value.slice(marker.length);
  if (!REFYARD_SUFFIX.test(suffix)) return null;

  return value as RefyardIdV1<Prefix>;
}

declare const deviceIdBrand: unique symbol;

/** Canonical `xdev_` text for Xross's nonzero 128-bit DeviceId. */
export type DeviceIdV1 = string & { readonly [deviceIdBrand]: true };

const DEVICE_ID_SUFFIX = /^[0-7][0-9a-hjkmnp-tv-z]{25}$/;

/** Requires the native host's canonical Display spelling, not parser aliases. */
export function parseCanonicalDeviceIdV1(value: string): DeviceIdV1 | null {
  const marker = "xdev_";
  if (!value.startsWith(marker)) return null;

  const suffix = value.slice(marker.length);
  if (!DEVICE_ID_SUFFIX.test(suffix) || /^0{26}$/.test(suffix)) return null;

  return value as DeviceIdV1;
}

export const SPACE_LENS_ID_PREFIX = {
  root: "xroot",
  node: "xnode",
  snapshot: "xsnapshot",
  scan: "xscan",
  job: "xjob",
  plan: "xplan",
  icloud: "xicloud",
} as const;

export type SpaceLensIdKind = keyof typeof SPACE_LENS_ID_PREFIX;

declare const spaceLensIdBrand: unique symbol;

/** A Space Lens view id with its distinct view-level domain tag. */
export type SpaceLensIdV1<Kind extends SpaceLensIdKind> = string & {
  readonly [spaceLensIdBrand]: Kind;
};

export type SpaceRootIdV1 = SpaceLensIdV1<"root">;
export type SpaceNodeIdV1 = SpaceLensIdV1<"node">;
export type SpaceSnapshotIdV1 = SpaceLensIdV1<"snapshot">;
export type SpaceScanIdV1 = SpaceLensIdV1<"scan">;
export type SpaceJobIdV1 = SpaceLensIdV1<"job">;
export type SpacePlanIdV1 = SpaceLensIdV1<"plan">;
export type SpaceICloudPlanIdV1 = SpaceLensIdV1<"icloud">;

const SPACE_LENS_SUFFIX = /^[0-9a-f]{32}$/;

/** Parses a domain-tagged 128-bit Xross view id without cross-domain coercion. */
export function parseSpaceLensId<Kind extends SpaceLensIdKind>(
  kind: Kind,
  value: string,
): SpaceLensIdV1<Kind> | null {
  const marker = `${SPACE_LENS_ID_PREFIX[kind]}_`;
  if (!value.startsWith(marker)) return null;

  const suffix = value.slice(marker.length);
  if (!SPACE_LENS_SUFFIX.test(suffix)) return null;

  return value as SpaceLensIdV1<Kind>;
}
