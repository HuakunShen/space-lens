/**
 * Vite resolves stylesheet imports from components (Svelte Flow ships its own
 * CSS); TypeScript only needs to know that the side-effect import is valid.
 */
declare module '*.css' {}
