/// <reference types="vite/client" />

// Vite resolves `?url` imports to the emitted asset path; declare it explicitly
// so it resolves cleanly through the `@core` alias.
declare module '@core/session_core_bg.wasm?url' {
  const src: string;
  export default src;
}
