// SvelteKit is routing only: no server routes exist anywhere in this app.
export const ssr = false
export const prerender = true
export const serviceWorker = !__SPACLENS_DESKTOP__ && !__SPACLENS_XROSS__
