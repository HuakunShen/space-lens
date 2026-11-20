export {
  ServiceError,
  createHttpService,
  type ConnectionState,
  type HttpService,
  type HttpServiceOptions,
} from './http.ts'
export { connectEventStream, type EventStreamOptions } from './events.ts'
export {
  TauriProblemError,
  createTauriRemotes,
  createTauriService,
  type DesktopSession,
  type RemoteDevice,
  type RemoteDevices,
  type RemoteInstallProgress,
  type RemoteSshHost,
  type RemoteSshListing,
  type SshTarget,
  type TauriPorts,
} from './tauri.ts'
export type { WorkbenchService } from './service.ts'
