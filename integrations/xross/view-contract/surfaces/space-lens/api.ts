import type {
  ApprovalResultV1,
  CursorV1,
  PageV1,
  SequencedViewEventV1,
  UInt64V1,
  UnixMillisV1,
} from "../../contracts/view-v1/types";
import type {
  SpaceICloudPlanIdV1,
  SpaceJobIdV1,
  SpaceNodeIdV1,
  SpacePlanIdV1,
  SpaceRootIdV1,
  SpaceScanIdV1,
  SpaceSnapshotIdV1,
} from "../../contracts/view-v1/ids";
import type { ViewContextV1 } from "../view-context";

export const SPACE_LENS_VIEW_METHODS_V1 = [
  "context",
  "capabilities",
  "listRoots",
  "startScan",
  "getScan",
  "watchScan",
  "cancelScan",
  "treeSlice",
  "childrenPage",
  "listAncestors",
  "listCandidates",
  "previewCleanup",
  "requestCleanupApproval",
  "planICloudEviction",
  "requestICloudEvictionApproval",
  "getCleanupJob",
  "watchCleanup",
  "controlICloudEviction",
] as const;

export interface SpaceLensCapabilitiesV1 {
  readonly apiMajor: number;
  readonly contractVersion: string;
  readonly scan: {
    readonly start: boolean;
    readonly cancel: boolean;
    readonly maxConcurrent: number;
  };
  readonly cleanup: {
    readonly plan: boolean;
    readonly execute: boolean;
    readonly mode: "none" | "trash";
  };
  readonly providers: {
    readonly iCloud: "available" | "unavailable";
  };
}

export interface SpaceLensListQueryV1 {
  readonly cursor?: CursorV1;
  readonly limit: number;
}
export type SpaceRootKindV1 = "volume" | "folder" | "multiFolder";
export interface RootSummaryV1 {
  readonly rootId: SpaceRootIdV1;
  readonly label: string;
  readonly kind: SpaceRootKindV1;
  readonly sizeBytes?: UInt64V1;
  readonly usedBytes?: UInt64V1;
  /** Xross target-derived features; not copied from the pinned Space Lens DTO. */
  readonly providerFeatures: readonly ("git" | "icloud" | "trash" | "filesystem")[];
}

export interface ScanOptionsV1 {
  readonly ignoreHidden: boolean;
  readonly respectGitignore: boolean;
  readonly ignoredMode: "summarize" | "exclude";
  readonly label?: string;
}
export interface StartScanRequestV1 {
  readonly rootIds: readonly SpaceRootIdV1[];
  readonly options: ScanOptionsV1;
}
export type ScanStateV1 = "queued" | "scanning" | "ready" | "cancelled" | "failed";
/** A target-authored binding from an approved root to this scan's snapshot root. */
export interface ScanRootNodeV1 {
  readonly rootId: SpaceRootIdV1;
  readonly nodeId: SpaceNodeIdV1;
}
export interface ScanStatusV1 {
  readonly scanId: SpaceScanIdV1;
  readonly state: ScanStateV1;
  readonly progressPermille?: number;
  readonly bytesVisited: UInt64V1;
  readonly entriesVisited: UInt64V1;
  readonly currentPathDisplay?: string;
  readonly rootIds: readonly SpaceRootIdV1[];
  readonly snapshotId?: SpaceSnapshotIdV1;
  /** Required only when ready; one distinct parentless node per approved root. */
  readonly rootNodes?: readonly ScanRootNodeV1[];
  readonly updatedAtUnixMs: UnixMillisV1;
}
export type ScanWatchEventV1 =
  | { readonly kind: "progress"; readonly scan: ScanStatusV1 }
  | { readonly kind: "ready"; readonly scan: ScanStatusV1 }
  | { readonly kind: "cancelled"; readonly scan: ScanStatusV1 }
  | { readonly kind: "failed"; readonly scan: ScanStatusV1; readonly error: "unavailable" | "limitExceeded" | "internal" };

export interface SnapshotNodeV1 {
  readonly nodeId: SpaceNodeIdV1;
  readonly parentId?: SpaceNodeIdV1;
  readonly name: string;
  readonly displayPath: string;
  readonly sizeBytes: UInt64V1;
  readonly depth: number;
  readonly childCount: number;
  readonly ignored: boolean;
  readonly collapsed: boolean;
  readonly hasChildren: boolean;
}
export interface TreeSliceV1 {
  readonly kind: "treeSlice";
  readonly snapshotId: SpaceSnapshotIdV1;
  readonly focusNodeId: SpaceNodeIdV1;
  readonly tree: readonly SnapshotNodeV1[];
  readonly ancestors: readonly SnapshotNodeV1[];
  readonly totalBytes: UInt64V1;
  readonly truncated: boolean;
  readonly omittedBytes: UInt64V1;
  readonly omittedCount: number;
  readonly generatedAtUnixMs: UnixMillisV1;
}
export interface ChildrenPageV1 {
  readonly kind: "childrenPage";
  readonly snapshotId: SpaceSnapshotIdV1;
  readonly nodeId: SpaceNodeIdV1;
  readonly items: readonly SnapshotNodeV1[];
  readonly nextCursor: CursorV1 | null;
  readonly total: number;
  readonly sort: "name" | "size" | "path";
}
export interface AncestorsPageV1 {
  readonly kind: "ancestorsPage";
  readonly snapshotId: SpaceSnapshotIdV1;
  readonly nodeId: SpaceNodeIdV1;
  readonly items: readonly SnapshotNodeV1[];
  readonly nextCursor: CursorV1 | null;
}
export type SnapshotPageV1 = TreeSliceV1 | ChildrenPageV1 | AncestorsPageV1;
export interface TreeSliceQueryV1 {
  readonly relativeDepth: number;
  readonly maxChildren: number;
  readonly maxNodes: number;
}
export interface ChildrenPageQueryV1 {
  readonly cursor?: CursorV1;
  readonly limit: number;
}

export interface CleanupCandidateV1 {
  readonly nodeId: SpaceNodeIdV1;
  readonly displayPath: string;
  readonly sizeBytes: UInt64V1;
  /** Closed localized projection; unknown source messages never cross the view. */
  readonly reasonCode: "ignored" | "staged" | "node" | "rust" | "gitignored" | "unknown";
  readonly preset?: "node" | "rust" | "gitignored";
}
export interface CleanupPlanV1 {
  readonly planId: SpacePlanIdV1;
  readonly snapshotId: SpaceSnapshotIdV1;
  readonly selected: readonly CleanupCandidateV1[];
  readonly totalBytes: UInt64V1;
  readonly mode: "trash";
  readonly createdAtUnixMs: UnixMillisV1;
  readonly expiresAtUnixMs: UnixMillisV1;
}
export interface ICloudPlanSummaryV1 {
  readonly planId: SpaceICloudPlanIdV1;
  readonly rootId: SpaceRootIdV1;
  readonly snapshotId: SpaceSnapshotIdV1;
  readonly selectedCount: number;
  readonly bytesToEvict: UInt64V1;
  readonly expiresAtUnixMs: UnixMillisV1;
}
export type SpaceJobStateV1 = "accepted" | "running" | "paused" | "succeeded" | "failed" | "cancelled" | "needsAttention";
export interface SpaceJobV1 {
  readonly jobId: SpaceJobIdV1;
  readonly state: SpaceJobStateV1;
  readonly sequence: UInt64V1;
  readonly completedCount: number;
  readonly totalCount: number;
  readonly completedBytes: UInt64V1;
  readonly totalBytes: UInt64V1;
  readonly outcomes: readonly { readonly code: "evicted" | "skipped" | "failed" | "unavailable"; readonly count: number }[];
}
export type CleanupWatchEventV1 = { readonly kind: "jobUpdated"; readonly job: SpaceJobV1 };
export type ICloudControlCommandV1 = "pause" | "resume" | "cancel";

export interface SpaceLensViewApiV1 {
  context(): Promise<ViewContextV1>;
  capabilities(): Promise<SpaceLensCapabilitiesV1>;
  listRoots(request: { readonly query: SpaceLensListQueryV1 }): Promise<PageV1<RootSummaryV1>>;
  startScan(request: StartScanRequestV1): Promise<ScanStatusV1>;
  getScan(request: { readonly scanId: SpaceScanIdV1 }): Promise<ScanStatusV1>;
  watchScan(request: { readonly scanId: SpaceScanIdV1; readonly sinceSequence?: UInt64V1 }): AsyncIterable<SequencedViewEventV1<ScanWatchEventV1>>;
  cancelScan(request: { readonly scanId: SpaceScanIdV1 }): Promise<ScanStatusV1>;
  treeSlice(request: { readonly snapshotId: SpaceSnapshotIdV1; readonly nodeId: SpaceNodeIdV1; readonly query: TreeSliceQueryV1 }): Promise<TreeSliceV1>;
  childrenPage(request: { readonly snapshotId: SpaceSnapshotIdV1; readonly nodeId: SpaceNodeIdV1; readonly query: ChildrenPageQueryV1 }): Promise<ChildrenPageV1>;
  listAncestors(request: { readonly snapshotId: SpaceSnapshotIdV1; readonly nodeId: SpaceNodeIdV1; readonly query: ChildrenPageQueryV1 }): Promise<AncestorsPageV1>;
  listCandidates(request: { readonly snapshotId: SpaceSnapshotIdV1; readonly query: SpaceLensListQueryV1 }): Promise<PageV1<CleanupCandidateV1>>;
  previewCleanup(request: { readonly snapshotId: SpaceSnapshotIdV1; readonly nodeIds: readonly SpaceNodeIdV1[] }): Promise<CleanupPlanV1>;
  requestCleanupApproval(request: { readonly planId: SpacePlanIdV1 }): Promise<ApprovalResultV1<SpaceJobV1>>;
  planICloudEviction(request: { readonly rootId: SpaceRootIdV1; readonly snapshotId: SpaceSnapshotIdV1 }): Promise<ICloudPlanSummaryV1>;
  requestICloudEvictionApproval(request: { readonly planId: SpaceICloudPlanIdV1 }): Promise<ApprovalResultV1<SpaceJobV1>>;
  getCleanupJob(request: { readonly jobId: SpaceJobIdV1 }): Promise<SpaceJobV1>;
  watchCleanup(request: { readonly jobId: SpaceJobIdV1; readonly sinceSequence?: UInt64V1 }): AsyncIterable<SequencedViewEventV1<CleanupWatchEventV1>>;
  controlICloudEviction(request: { readonly jobId: SpaceJobIdV1; readonly command: ICloudControlCommandV1 }): Promise<SpaceJobV1>;
}

type SpaceLensApiMethodNameV1 = (typeof SPACE_LENS_VIEW_METHODS_V1)[number];
type SpaceLensMethodListIsExactV1 = Exclude<keyof SpaceLensViewApiV1, SpaceLensApiMethodNameV1> extends never
  ? Exclude<SpaceLensApiMethodNameV1, keyof SpaceLensViewApiV1> extends never
    ? true
    : false
  : false;
const _spaceLensMethodListIsExact: SpaceLensMethodListIsExactV1 = true;
void _spaceLensMethodListIsExact;
