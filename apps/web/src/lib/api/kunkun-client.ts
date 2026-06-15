import type {
  ChildrenPage,
  CleanupOutcome,
  RemovalPlan,
  ScanSession,
  ScanStatus,
  SpaceLensAPI,
  TreeSlice,
} from "./types";

const message =
  "Kunkun transport is not wired yet. Use the standalone KKRPC WebSocket server for now.";

export function createKunkunClient(): SpaceLensAPI {
  return {
    startScan: () => unavailable<ScanSession>(),
    getNode: () => unavailable<TreeSlice>(),
    getChildren: () => unavailable<ChildrenPage>(),
    getScanStatus: () => unavailable<ScanStatus>(),
    cancelScan: () => unavailable<void>(),
    planCleanup: () => unavailable<RemovalPlan>(),
    executeCleanup: () => unavailable<CleanupOutcome>(),
  };
}

function unavailable<T>(): Promise<T> {
  return Promise.reject(new Error(message));
}
