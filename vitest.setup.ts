import { readFileSync } from 'fs';
import { resolve } from 'path';
import { initSync } from '@core/session_core.js';

// Tests run in Node, where the wasm-bindgen `web` target's fetch-based init
// can't resolve a `?url` asset path. Load the binary directly instead; this
// populates the module-level wasm instance that the generated bindings call
// into directly, so initSessionCore()'s later `init(wasmUrl)` short-circuits.
initSync({ module: readFileSync(resolve(__dirname, 'session-core/pkg/session_core_bg.wasm')) });
