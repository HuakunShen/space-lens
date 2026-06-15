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

export interface ScanTarget {
  id: string;
  label: string;
  path: string;
  kind: "volume" | "folder" | "multi-folder";
  description: string;
  size: number;
  used?: number;
}
