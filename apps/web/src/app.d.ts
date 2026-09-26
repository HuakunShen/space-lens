declare global {
  // Set by vite define from SPACLENS_BUILD_TARGET; the desktop flavor must not
  // register a service worker and may rely on the Tauri IPC adapter.
  var __SPACLENS_DESKTOP__: boolean
  var __SPACLENS_XROSS__: boolean
}

export {}
