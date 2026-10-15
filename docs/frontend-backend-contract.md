# What crosses between the frontend and the engine

The frontend holds no state the engine owns and runs no logic the engine runs. Everything below
is how that is kept true in practice, and why the shapes differ where they do.

## The two rules underneath everything

**The engine decides, the frontend mirrors.** A transport verb does not tell the deck what to
become; it asks the engine to act and reports what the engine now is. Anything the frontend
recomputes from a return value is a place the two can disagree.

**One implementation, two runners.** The live engine and the session simulation must apply a
command the same way, so that logic lives in `session-core` and is compiled twice: natively for
`src-tauri`, and to WASM for the frontend. What decides which side something belongs on is not
"is it logic" but **does it exist in both the live path and the replay path**. A MIDI mapping
does not, so it is a Tauri command. Clip edits, lane automation and the timeline do, so they are
WASM.

## Six kinds of command

Every `#[tauri::command]` is one of these. Adding one means choosing which, deliberately.

### 1. The verb returns the resulting state

`toggle_play`, `press_cue`, `release_cue`, `seek`, `set_loop_in`, `set_loop_out`,
`set_loop_active`, `set_reloop` return a `DeckSyncPayload`, and `applyDeckState` assigns it
wholesale. The frontend reads no field to decide anything; it copies. `set_loop_out` returns
`Option<DeckSyncPayload>`, where `None` is the only thing the caller decides: the press defined
no region.

The MIDI learn commands are the same shape over a different subject. `start_midi_learn`,
`arm_midi_slot`, `disarm_midi_slot`, `clear_midi_slot` and `set_midi_draft_meta` each return the
whole `MappingDraft`, including which slot is armed, because the armed slot is what routes an
incoming message and only one side can own that.

Several of these have a `_core` function under them that both the live engine and the offline
renderer call. `loop_out_core` is the example: it decides the quantized loop bounds once, the
live path turns its `LoopOutResult` into a recorder event and the renderer turns the same struct
into the same `.bms` event. That is "one implementation, two runners" again, inside Rust rather
than across the WASM boundary, and it is why `LoopOutResult` still exists after `set_loop_out`
stopped returning it. A type that no longer crosses IPC is not automatically dead; check whether
the replay path reads it.

**Prefer this kind.** It is the only one with no room for the two sides to drift.

### 2. The verb returns nothing, and the mirror arrives on a push

`set_deck_param`, `set_master_gain`, `set_xfader_position`, `set_quantize`, `set_cue_active`,
`set_beat_grid`, `set_playback_rate`. These cannot be kind 1, and that is deliberate rather than
an omission: `engine_push.rs` skips a `ParamOrigin::Ui` write, because the UI already shows the
change it just made and echoing it would drag a fader out from under the pointer mid-drag. Only
a write the UI did not make comes back, batched every 16 ms as an address rather than a value.

The truth is still the engine's. The round trip is asynchronous and one-directional.

The learn panel's live capture works this way: `midi-learn` carries what the moving control
currently reads as, which is a value being pushed, not state being owned.

### 3. The verb returns a result that is not a state

`set_nudge` returns a `NudgeResult`, `set_pitch_offset` an `f64`. These report what the call
produced rather than what the deck now is. Every one of these is a candidate for kind 1 and
should be looked at with suspicion; `set_loop_out` used to be here, returning a region plus a
`seekToSec` the store applied by hand, and moving it to kind 1 deleted the only place in the
transport path where the frontend rebuilt state from parts.

### 4. Polls

`get_master_level`, `get_deck_levels`, `recording_save_progress`, `list_audio_devices`,
`list_recoverable`. Read-only snapshots on a timer or on demand.

### 5. Bulk pulls

`get_dense_points` returns a raw `ipc::Response` of bytes; `get_spectral_waveform_region` and
`get_track_amplitude_region` return sampled regions. Shaped for volume. State mirroring does not
apply, and neither does the typed `call` wrapper for the first, which is an `ArrayBuffer`.

### 6. Files, dialogs and lifecycle

`pick_save_path`, `open_session_dialog`, `save_session`, `save_midi_mapping`, `scan_folder`,
`read_file`, `load_track`, `preload_session`, `confirm_quit`.

## What the frontend is still allowed to own

Interaction state, and only that: held keys, drag gestures, selection, view and zoom. Plus two
derived displays that are explicitly the frontend's, both marked at their definition:

- `positionCache` and `clockAtPlay` in `decks.ts` interpolate a playhead between syncs. Every
  kind 1 payload resets both, so the interpolation is never the source of a position.
- `targetBpm` and `pitchOffset` are recomputed from the rate the engine reports, because the
  engine owns the rate and the display of it is a rounding decision (`roundBpm`, two decimals,
  so a shown bpm is exactly settable).

## Typing what crosses

`tauriCommands.ts` is generated from the `#[tauri::command]` signatures and its `call` wrapper
checks the name, the arguments and the return against the Rust that will receive them. A struct
resolves only if the generator's `MIRRORS` table names it; anything else is `unknown`, which is
the signal that nothing is checking it.

A mirror has to live under `utils/`. Stores import the generated file, so the generated file
importing a store would close a cycle that `yarn circular-deps` counts, type-only edges included.

`unknown` in a generated signature is a defect, not a style. An `unknown` **argument** is worse
than an `unknown` return, because Rust often degrades a bad value silently rather than failing
(`FaderCurve::from_str_or_linear`, `JogRotationSpeed::from_str_or_33`).

### Types generated from Rust

`src/renderer/src/generated/` holds TypeScript written by `ts-rs` from the Rust structs, produced
by `yarn generate:bindings` and committed the way `tauriCommands.ts` is, so a typecheck needs no
Rust toolchain. A struct opts in with `#[derive(ts_rs::TS)]` and an `#[ts(export, export_to =
"../../src/renderer/src/generated/")]`, and the export runs as a test, so `cargo test` regenerates
it. `export_to` is relative to `src-tauri/bindings`, not to the file.

A generated type beats a hand-written mirror because a renamed Rust field then fails the
TypeScript build instead of silently producing a wrong shape. It also reports what the Rust
actually says rather than what someone assumed: exporting `DeckSyncPayload` turned up a `beats:
i64` that the hand-written mirror called `number`, which is only right because the value is small.
`i64` has no exact JSON representation, so the field became `i32`, which is what it always was.

Prefer fixing the Rust type over overriding the generator with `#[ts(type = "...")]`. An override
is a note saying the two disagree.

## Where the two sources of truth are, today

Two facts are reachable by two paths, and both are known:

- The mixer manifest arrives through WASM as `mixerParams(LIVE_MIXER_ID)`, and the MIDI
  vocabulary separately reports which of its addresses a control change can reach. The overlap is
  the slot and param names; the filter is MIDI's own concern, so this is tolerated.
- `LIVE_MIXER_ID` is a constant in `settings.ts`. The frontend asserts which manifest the engine
  builds rather than being told, and nothing pins the two together.
