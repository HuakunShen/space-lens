/**
 * Standalone Space Lens builds use this stub so the shared web app can compile
 * without depending on Kunkun's unpublished SDK package.
 */
import type { KunkunRuntimeAdapter } from "./kunkun-runtime";

export type KunkunCustomApiModule = {
  readonly LocalStorage: KunkunRuntimeAdapter["storage"];
  confirmAlert: KunkunRuntimeAdapter["confirmAlert"];
  showInFinder: KunkunRuntimeAdapter["showInFinder"];
  trash: KunkunRuntimeAdapter["trash"];
  spawnBackend: KunkunRuntimeAdapter["spawnBackend"];
};

export type KunkunApiModule = {
  readonly path: KunkunRuntimeAdapter["path"];
  readonly permissions: KunkunRuntimeAdapter["permissions"];
};

export type KunkunSdkModules = {
  readonly customApi: KunkunCustomApiModule;
  readonly api: KunkunApiModule;
};

export async function loadKunkunSdkModules(): Promise<KunkunSdkModules> {
  throw new Error(
    "Space Lens was built without Kunkun SDK bindings. Rebuild through apps/kunkun-plugin for Kunkun custom-view mode.",
  );
}
