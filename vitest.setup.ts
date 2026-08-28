import { vi } from 'vitest';
import { readFileSync } from 'fs';
import { resolve } from 'path';
import { initSync } from '@core/session_core.js';

// Tests run in Node, where the wasm-bindgen `web` target's fetch-based init
// can't resolve a `?url` asset path. Load the binary directly instead. This
// populates the module-level wasm instance that the generated bindings call
// into directly, so initSessionCore()'s later `init(wasmUrl)` short-circuits.
initSync({ module: readFileSync(resolve(__dirname, 'session-core/pkg/session_core_bg.wasm')) });

// Tests run outside the Tauri webview, where `listen` reaches for an IPC
// bridge that does not exist. Stores that subscribe at creation would print a
// rejected promise on every suite that touches them.
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
  emit: vi.fn().mockResolvedValue(undefined)
}));
