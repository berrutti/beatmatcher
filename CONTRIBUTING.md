# Contributing

Thanks for considering a contribution to Beatmatcher.

## Setup

```bash
yarn install
yarn dev
```

See [architecture.md](architecture.md) for an overview of the audio engine, signal chain, and IPC layer between the Rust backend and the Vue frontend.

## Before opening a PR

- Keep PRs focused on a single change. Unrelated cleanups make review harder
- Remember to keep the format of the project by running the appropriate scripts. When creating a PR, the CI will tell you if the code can be merged. Failing PRs cannot be considered
- If you touched DSP, loop timing, or quantization logic, that logic belongs in Rust. The frontend should only mirror results returned by Tauri commands, never reimplement it
- If you touched the session event format (`.bms`) or the `session-core` crate, update [docs/bms-format.md](docs/bms-format.md) or [docs/session-playback.md](docs/session-playback.md) accordingly

## Reporting bugs

Open an issue with steps to reproduce, your OS, and the Beatmatcher version. If the bug involves audio glitches or crashes, mention your audio device setup (main/cue device split, buffer size).
