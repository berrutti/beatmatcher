# Session playback: scrubbing and snapshot state machine

On session open, `preload_session` decodes all referenced audio files into a persistent cache and builds a `SessionSnapshot` after every event in the session log. Each snapshot captures the complete state of all decks (position, rate, nudge, loop) and all channel strips (gain, EQ, filter).

When `start_session_playback(fromMs)` is called the scheduler:

1. Finds the last snapshot with `elapsed_ms ≤ fromMs` (binary search)
2. Resets all decks and strips
3. Applies the snapshot's strip/master state to the live engine
4. Replays the few events between the snapshot and `fromMs` through the analytical position simulation
5. Sets final deck positions (all decks start on the same output buffer) and schedules the remaining events against the master output-frame clock, compensating a deck that enters mid-playback for sub-buffer start latency

Position simulation rule: any event that changes playback speed or position (`play`, `stop`, `seek`, `set_playback_rate`, `set_nudge`) commits the current analytically-computed position before updating the relevant parameter. This ensures each segment between events is computed with the correct rate and nudge factor.

## Session editing

Sessions are edited in memory. The frontend's parsed event array is the source of truth. Every edit produces a new array (reference equality drives the dirty flag and undo/redo) and is pushed to Rust via `update_session_events`, which rebuilds the snapshot state machine. Saving serializes from the frontend's raw JSON so unknown fields survive a round-trip. The .bms on disk is untouched until the changes are saved.

Two kinds of edits exist, both implemented as pure functions over the event array in `session-core` (Rust, compiled to WASM) and called from the frontend through the `sessionCore.ts` wrapper.

- **Automation lane edits** (`lane_edit.rs`): drawing on a lane splices new `set_*` events into the drawn range and restores the original value at the range end. The same range-paint primitive backs filter-region edits (toggle/resize/move/delete an active span) and jog-lane painting: each paint replaces its span with one value, then restores whatever the value was at the release point. Right-clicking a lane resets it instead of drawing on it: `reset_lane_from` clears it from the click to the end, up to the click, or just the move under the cursor, where a move is the run of events on one side of the lane's default, bounded by the crossings either side of it (`lane_move_span`, which the timeline also reads to highlight what the reset would clear). Right-clicking a clip's waveform also sets its tempo by writing `set_playback_rate` directly: `set_rate_at` inserts one rate change at the click (held until the next change) and `set_rate_span` makes a whole clip one tempo and restores the prior rate after it, both converting the entered BPM via the clip's recorded grid bpm.
- **Clip edits** (`clip_edit.rs`): moving, trimming, or deleting a play segment rewrites deck-transport events. A moved block is deleted from its old position and re-synthesized as a self-contained `play {sec}` … `stop` pair (loops get `loop_out`/`exit_loop` around it). Deleting a block drops its transport events and, when no other clip plays from its `load_track`, it drops that orphaned load too. Deletion is range-based (`delete_transport_ranges`): a range covering a whole block is a full delete, a range touching an edge is a trim, and an interior range splits the block in two, the right part resuming on the exact audio position integrated over the rate/nudge curve. Loop blocks re-engage at the deck's in-loop position, so surviving iterations keep their phase. Every synthesized value comes from `buildClips` output, so rewriting one block's boundaries can never change a neighboring clip's audio. Automation events stay at wall time. Two ordering rules keep edited streams unambiguous: at an exactly equal timestamp (only edits synthesize those) transport enders sort before starters, and a `deck_snapshot` sorts first within its rounded-millisecond cluster. Transport events stranded in silence (e.g. a seek recorded while a deck was paused) are consumed when a moved or extended block covers them.

On the Rust side, `session-core/src/event.rs` parses each raw event into the closed `SessionCommand` enum. The three interpreters (scrub simulation, live scheduler, offline renderer) match on it exhaustively with no catch-all, so adding an event type fails compilation until each interpreter decides its behavior.
