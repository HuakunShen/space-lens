/**
 * Kunkun plugin builds alias the SDK loader to this file so Vite can statically
 * discover and bundle the Kunkun SDK chunks used by custom-view mode.
 */
import type { KunkunApiModule, KunkunSdkModules } from "./kunkun-sdk-loader";

type KunkunCustomApiModule = KunkunSdkModules["customApi"];

export async function loadKunkunSdkModules(): Promise<KunkunSdkModules> {
  const [customApi, api] = await Promise.all([
    import("@kunkunsh/sdk/ui/custom") as Promise<KunkunCustomApiModule>,
    import("@kunkunsh/sdk") as Promise<KunkunApiModule>,
  ]);

  return { customApi, api };
}
