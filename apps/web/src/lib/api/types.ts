export type {
  ChildrenPage,
  CleanupOutcome,
  CleanupPlanOptions,
  CollectorEntry,
  ExecuteCleanupOptions,
  GetChildrenRequest,
  GetNodeRequest,
  RemovalEntry,
  RemovalPlan,
  ScanSession,
  ScanStatus,
  ScanTarget,
  SortMode,
  SpaceLensAPI,
  StartScanOptions,
  TreeNodeSummary,
  TreeSlice,
  TreeSliceNode,
} from "@space-lens/cli/web-api";

export type RuntimeMode = "demo" | "rpc" | "kunkun";

export interface CollectorState {
  entries: import("@space-lens/cli/web-api").CollectorEntry[];
  totalSize: number;
}
