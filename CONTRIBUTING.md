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

## Controller mappings

A **mapping** is a JSON file in `src-tauri/mappings/`, listing bindings that each pair a **source** with an **action**:

- A source is a control change or a note. Its resolution says how to read the value: `7bit`, `14bit` (low half on the controller 32 above the high one), `centre_delta` (signed speed either side of 64, what a jog platter reports), or `signed_step` (1 or 127 per detent, what a browse encoder reports).
- An action is one entry in a closed enum: a mixer parameter addressed as deck/slot/param, the crossfader, a transport button, the tempo fader, the jog, or browse.

`channel` counts from 1, the way the hardware's documentation and the console monitor do. The wire counts from 0 and the loader subtracts.

`match` claims a device by port-name substring. `decks` is `fixed` when the surface names its own decks and `assigned` when the user picks one deck for the whole device.

Most surfaces lay the same controls out per deck, one MIDI channel each. `per_deck` maps each deck to its channel and `deck_bindings` are written once and expanded across it, so neither the deck nor the channel is retyped per copy. That matters because a mistyped deck letter is a _valid_ address on another deck, which the collision check cannot see and which reads as the wrong deck's control moving. Anything that does not fit the pattern stays in `bindings` with its channel and deck written out.

A positional control is placed on its parameter's range by the mixer manifest (`ParamDescriptor::from_unit_interval`), so a binding carries no range of its own. A mapping whose bindings collide on one key is refused, because a silent collision moves the wrong deck's control and reads as broken hardware.

### Contributing one

Bind against the device rather than its documentation. Dev builds log every incoming message to the browser console as raw bytes plus a decode, so connecting a controller in Settings and moving one control at a time is how addresses are found.

Encoders are the trap. Read one through a **full revolution** before deciding what it reports, because an absolute angle, a signed step and a speed are indistinguishable over a small nudge.

Capture every control twice, once with the surface's modifier held and once without. A modifier can move a control to a different address rather than change what it means, so a mapping that binds only the unmodified address loses the control entirely while the modifier is held.

A control no action covers needs a new enum variant, and adding one fails to compile until every interpreter handles it. The vocabulary a mapping file can address should grow deliberately.

`version` is bumped when the file shape gains something an older build cannot see. A file declaring a newer version is refused, and so is one that uses the deck template while declaring a version that predates it, because the older build would find no bindings and present a dead controller rather than an error.

## Platform notes

- Pin `@tauri-apps/api`, `@tauri-apps/cli` and the `tauri` crate to the same explicit minor. `^2` lets them resolve apart and the build fails before compiling.
- `dragDropEnabled: false` in `tauri.conf.json` is required for HTML5 file drops.
- On the macOS About dialog, never set `.version()` on `AboutMetadataBuilder`. URLs go in `.copyright()` as plain text.
- Two simultaneous output streams (master and cue) work on PipeWire. Bare ALSA needs dmix.
- A release creates the GitHub Release first, then each platform's build uploads to it. Creating it per job raced once there were three platforms.

## Reporting bugs

Open an issue with steps to reproduce, your OS, and the Beatmatcher version. If the bug involves audio glitches or crashes, mention your audio device setup (main/cue device split, buffer size).
