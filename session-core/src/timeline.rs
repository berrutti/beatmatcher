// Timeline derivation: turns the event stream into the editor's visual model.
// `build_clips` produces the per-deck playing segments (one clip per loop
// iteration) and the loaded-track spans; `build_lanes` produces the automation
// lanes (gain/eq/filter/rate) plus filter-active and nudge spans.
//
// This is a faithful port of the frontend's useSessionTimeline.ts so the editor
// can run the SAME derivation via WASM instead of a divergent TS copy. Track
// display names are intentionally NOT derived here: the path is returned and the
// frontend maps it to a collection title.

use crate::event::SessionEvent;
use crate::sim::DEFAULT_MASTER_GAIN;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopRegion {
    pub start_sec: f64,
    pub end_sec: f64,
}

// Clips emitted together form one editable unit: loop iterations share a
// block_id; a regular play segment is a block of its own.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub deck: String,
    pub session_start_ms: f64,
    pub session_end_ms: f64,
    pub track_path: String,
    pub track_start_sec: f64,
    pub playback_rate: f64,
    pub block_id: u32,
    #[serde(rename = "loop")]
    pub loop_region: Option<LoopRegion>,
    // The clip's wall span split into constant-effective-rate (rate*nudge)
    // pieces, each mapping a track-time window to a wall-time window. Drawing the
    // waveform/beats per segment is what makes them compress/stretch correctly
    // when the rate or nudge changes mid-clip.
    #[serde(default)]
    pub wave_segments: Vec<WaveSeg>,
    // Beat grid in effect when the clip started (recorded, not the live
    // collection value). `None` bpm means no grid was known: draw no beats.
    #[serde(default)]
    pub bpm: Option<f64>,
    #[serde(default)]
    pub beat_offset_sec: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaveSeg {
    pub wall_start_ms: f64,
    pub wall_end_ms: f64,
    pub track_start_sec: f64,
    pub track_end_sec: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedSpan {
    pub deck: String,
    pub track_path: String,
    pub start_ms: f64,
    pub end_ms: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipsBuild {
    pub clips: Vec<Clip>,
    pub loaded_spans: Vec<LoadedSpan>,
}

#[derive(Default)]
struct DeckState {
    path: Option<String>,
    track_pos_sec: f64,
    pos_mark_ms: f64,
    rate: f64,
    nudge_factor: f64,
    loop_start_sec: Option<f64>,
    loop_end_sec: Option<f64>,
    loop_active: bool,
    loop_engaged_ms: Option<f64>,
    loop_entry_sec: Option<f64>,
    clip_start_ms: Option<f64>,
    clip_track_start_sec: f64,
    clip_rate: f64,
    clip_path: Option<String>,
    clip_bpm: Option<f64>,
    clip_beat_offset_sec: f64,
    load_span_start_ms: Option<f64>,
    load_span_path: Option<String>,
    bpm: Option<f64>,
    beat_offset_sec: f64,
    // (wall_ms, effective_rate) whenever rate or nudge changed, in event order.
    eff_rate_changes: Vec<(f64, f64)>,
}

fn make_deck_state() -> DeckState {
    DeckState {
        rate: 1.0,
        nudge_factor: 1.0,
        clip_rate: 1.0,
        ..Default::default()
    }
}

// Mirrors the engine's position stepping (playback_rate * nudge_factor,
// wrapping inside an active loop). Without this, a bare resume `play` after a
// stop would inherit a stale position from the last explicit position event,
// and clip edits would bake that wrong position into synthesized events.
fn advance_position(deck: &mut DeckState, ms: f64) {
    let playing = deck.clip_start_ms.is_some() || deck.loop_active;
    if playing && ms > deck.pos_mark_ms {
        let mut pos =
            deck.track_pos_sec + ((ms - deck.pos_mark_ms) / 1000.0) * deck.rate * deck.nudge_factor;
        if deck.loop_active {
            if let (Some(ls), Some(le)) = (deck.loop_start_sec, deck.loop_end_sec) {
                let duration = le - ls;
                if duration > 0.0 && pos >= le {
                    pos = ls + ((pos - le) % duration);
                }
            }
        }
        deck.track_pos_sec = pos;
    }
    deck.pos_mark_ms = ms;
}

// Record the effective rate (rate*nudge) at `ms` so finalize_clip can slice a
// clip's wall span into constant-rate wave segments. Call after any change to
// `deck.rate` or `deck.nudge_factor`.
fn record_eff_rate(deck: &mut DeckState, ms: f64) {
    deck.eff_rate_changes.push((ms, deck.rate * deck.nudge_factor));
}

// Effective rate in force at `ms` (the last recorded change at/before it).
fn eff_rate_at(changes: &[(f64, f64)], ms: f64) -> f64 {
    let mut rate = 1.0;
    for &(t, r) in changes {
        if t <= ms {
            rate = r;
        } else {
            break;
        }
    }
    rate
}

// Slice [clip_start_ms, clip_end_ms] at each rate change inside it, integrating
// the track position forward at each piece's effective rate. The piece track
// deltas sum to the same end position advance_position produced, so the segments
// tile the clip exactly.
fn wave_segments_for(
    changes: &[(f64, f64)],
    clip_start_ms: f64,
    clip_end_ms: f64,
    track_start_sec: f64,
) -> Vec<WaveSeg> {
    if clip_end_ms <= clip_start_ms {
        return Vec::new();
    }
    let mut bounds = vec![clip_start_ms];
    for &(t, _) in changes {
        if t > clip_start_ms && t < clip_end_ms {
            bounds.push(t);
        }
    }
    bounds.push(clip_end_ms);
    bounds.dedup();

    let mut segs = Vec::new();
    let mut track = track_start_sec;
    for pair in bounds.windows(2) {
        let (w0, w1) = (pair[0], pair[1]);
        let track_end = track + ((w1 - w0) / 1000.0) * eff_rate_at(changes, w0);
        segs.push(WaveSeg {
            wall_start_ms: w0,
            wall_end_ms: w1,
            track_start_sec: track,
            track_end_sec: track_end,
        });
        track = track_end;
    }
    segs
}

fn start_clip(deck: &mut DeckState, ms: f64) {
    deck.clip_start_ms = Some(ms);
    deck.clip_track_start_sec = deck.track_pos_sec;
    deck.clip_rate = deck.rate;
    deck.clip_path = deck.path.clone();
    deck.clip_bpm = deck.bpm;
    deck.clip_beat_offset_sec = deck.beat_offset_sec;
}

fn engage_loop(deck: &mut DeckState, ms: f64) {
    deck.loop_active = true;
    deck.loop_engaged_ms = Some(ms);
    deck.clip_path = deck.path.clone();
    deck.clip_rate = deck.rate;
    deck.clip_bpm = deck.bpm;
    deck.clip_beat_offset_sec = deck.beat_offset_sec;
    deck.clip_start_ms = Some(ms);
    // The engine never jumps to the loop start when a loop engages: a playhead
    // already past the end (late quantized loop_out press) wraps its overshoot
    // into the region, anywhere else it keeps playing from where it is.
    if let (Some(ls), Some(le)) = (deck.loop_start_sec, deck.loop_end_sec) {
        let dur = le - ls;
        if dur > 0.0 && deck.track_pos_sec >= le {
            deck.track_pos_sec = ls + ((deck.track_pos_sec - le) % dur);
        }
    }
    deck.loop_entry_sec = Some(deck.track_pos_sec);
}

fn finalize_clip(
    deck: &mut DeckState,
    deck_id: &str,
    end_ms: f64,
    out: &mut Vec<Clip>,
    next_block_id: &mut u32,
) {
    let mut allocate = || {
        let id = *next_block_id;
        *next_block_id += 1;
        id
    };

    if deck.loop_active && deck.loop_engaged_ms.is_some() && deck.clip_path.is_some() {
        if let (Some(loop_start_sec), Some(loop_end_sec)) = (deck.loop_start_sec, deck.loop_end_sec)
        {
            let loop_dur_sec = loop_end_sec - loop_start_sec;
            if loop_dur_sec > 0.0 && deck.clip_rate > 0.0 {
                let loop_path = deck.clip_path.clone().unwrap();
                let loop_rate = deck.clip_rate;
                let block_id = allocate();
                let mut iter_start = deck.loop_engaged_ms.unwrap();
                // The first iteration starts at the wrapped entry position, which may
                // be inside the region; every later iteration runs the full loop.
                let mut iter_track_start_sec = deck.loop_entry_sec.unwrap_or(loop_start_sec);
                while iter_start < end_ms {
                    let iter_dur_ms = ((loop_end_sec - iter_track_start_sec) / loop_rate) * 1000.0;
                    if iter_dur_ms <= 0.0 {
                        break;
                    }
                    let iter_end = (iter_start + iter_dur_ms).min(end_ms);
                    // One segment per iteration: the loop runs at a constant rate,
                    // so the iteration's track window maps linearly to its wall window.
                    let iter_track_end =
                        iter_track_start_sec + ((iter_end - iter_start) / 1000.0) * loop_rate;
                    out.push(Clip {
                        deck: deck_id.to_string(),
                        session_start_ms: iter_start,
                        session_end_ms: iter_end,
                        track_path: loop_path.clone(),
                        track_start_sec: iter_track_start_sec,
                        playback_rate: loop_rate,
                        block_id,
                        loop_region: Some(LoopRegion {
                            start_sec: loop_start_sec,
                            end_sec: loop_end_sec,
                        }),
                        wave_segments: vec![WaveSeg {
                            wall_start_ms: iter_start,
                            wall_end_ms: iter_end,
                            track_start_sec: iter_track_start_sec,
                            track_end_sec: iter_track_end,
                        }],
                        bpm: deck.clip_bpm,
                        beat_offset_sec: Some(deck.clip_beat_offset_sec),
                    });
                    iter_start += iter_dur_ms;
                    iter_track_start_sec = loop_start_sec;
                }
            }
        }
        deck.loop_active = false;
        deck.loop_engaged_ms = None;
        deck.loop_entry_sec = None;
        deck.clip_start_ms = None;
    } else if let Some(clip_start) = deck.clip_start_ms {
        // A zero-length clip emits nothing, but the deck still stopped: leaving
        // clip_start_ms set would swallow the next play (the engine's stop always
        // stops, regardless of how long the clip was).
        if let Some(clip_path) = &deck.clip_path {
            if end_ms > clip_start {
                let wave_segments = wave_segments_for(
                    &deck.eff_rate_changes,
                    clip_start,
                    end_ms,
                    deck.clip_track_start_sec,
                );
                out.push(Clip {
                    deck: deck_id.to_string(),
                    session_start_ms: clip_start,
                    session_end_ms: end_ms,
                    track_path: clip_path.clone(),
                    track_start_sec: deck.clip_track_start_sec,
                    playback_rate: deck.clip_rate,
                    block_id: allocate(),
                    loop_region: None,
                    wave_segments,
                    bpm: deck.clip_bpm,
                    beat_offset_sec: Some(deck.clip_beat_offset_sec),
                });
            }
        }
        deck.clip_start_ms = None;
    }
}

fn finalize_loaded_span(
    deck: &mut DeckState,
    deck_id: &str,
    end_ms: f64,
    out: &mut Vec<LoadedSpan>,
) {
    let (Some(start_ms), Some(path)) = (deck.load_span_start_ms, deck.load_span_path.clone())
    else {
        return;
    };
    out.push(LoadedSpan {
        deck: deck_id.to_string(),
        track_path: path,
        start_ms,
        end_ms,
    });
    deck.load_span_start_ms = None;
    deck.load_span_path = None;
}

// Shared sequence for all loop-exit events: track_pos_sec already holds the
// in-loop position (advance_position wraps while the loop is active), so just
// finalize the loop iterations as clips and start a new regular clip.
fn exit_loop_and_continue(
    deck: &mut DeckState,
    deck_id: &str,
    ms: f64,
    clips: &mut Vec<Clip>,
    next_block_id: &mut u32,
) {
    finalize_clip(deck, deck_id, ms, clips, next_block_id);
    start_clip(deck, ms);
}

pub fn build_clips(events: &[SessionEvent]) -> ClipsBuild {
    let mut deck_states: BTreeMap<String, DeckState> = BTreeMap::new();
    let mut clips: Vec<Clip> = Vec::new();
    let mut loaded_spans: Vec<LoadedSpan> = Vec::new();
    let mut next_block_id: u32 = 0;

    for ev in events {
        let Some(deck_id) = ev.deck.as_deref() else {
            continue;
        };
        let deck = deck_states
            .entry(deck_id.to_string())
            .or_insert_with(make_deck_state);
        advance_position(deck, ev.elapsed_ms);

        match ev.event_type.as_str() {
            "deck_snapshot" => {
                deck.path = ev.path.clone();
                deck.rate = ev.playback_rate.unwrap_or(1.0);
                if let Some(bpm) = ev.bpm {
                    deck.bpm = Some(bpm);
                }
                record_eff_rate(deck, ev.elapsed_ms);
                deck.track_pos_sec = ev.position_sec.unwrap_or(0.0);
                // loop start = cue_point by invariant; deck_snapshot logs
                // cue_point_sec, not loop_start_sec.
                let loop_on = ev.loop_active == Some(true);
                deck.loop_start_sec = if loop_on { ev.cue_point_sec } else { None };
                deck.loop_end_sec = if loop_on { ev.loop_end_sec } else { None };
                if deck.path.is_some() && deck.load_span_start_ms.is_none() {
                    deck.load_span_start_ms = Some(0.0);
                    deck.load_span_path = deck.path.clone();
                }
                if ev.is_playing == Some(true) {
                    if loop_on && deck.loop_start_sec.is_some() && deck.loop_end_sec.is_some() {
                        engage_loop(deck, ev.elapsed_ms);
                    } else {
                        start_clip(deck, ev.elapsed_ms);
                    }
                }
            }

            "load_track" => {
                finalize_clip(deck, deck_id, ev.elapsed_ms, &mut clips, &mut next_block_id);
                finalize_loaded_span(deck, deck_id, ev.elapsed_ms, &mut loaded_spans);
                deck.path = ev.path.clone();
                deck.load_span_start_ms = Some(ev.elapsed_ms);
                deck.load_span_path = deck.path.clone();
                deck.track_pos_sec = 0.0;
                // The engine fully resets the deck on load (playback_rate = 1.0 in
                // load_track), and the sim mirrors it; recorded sessions re-seed the
                // rate right after, but an edited stream may not.
                deck.rate = 1.0;
                deck.nudge_factor = 1.0;
                // A freshly loaded track has no grid until set_beat_grid/analyze.
                deck.bpm = None;
                deck.beat_offset_sec = 0.0;
                record_eff_rate(deck, ev.elapsed_ms);
                deck.loop_start_sec = None;
                deck.loop_end_sec = None;
                deck.loop_active = false;
                deck.loop_engaged_ms = None;
            }

            "eject_track" => {
                finalize_clip(deck, deck_id, ev.elapsed_ms, &mut clips, &mut next_block_id);
                finalize_loaded_span(deck, deck_id, ev.elapsed_ms, &mut loaded_spans);
                deck.path = None;
                deck.track_pos_sec = 0.0;
                deck.loop_active = false;
                deck.loop_engaged_ms = None;
            }

            "play" => {
                // Edits synthesize play-with-sec; the engine teleports on it
                // even while playing, so an open clip splits like a seek.
                if let Some(sec) = ev.sec {
                    if deck.clip_start_ms.is_some() && !deck.loop_active {
                        finalize_clip(deck, deck_id, ev.elapsed_ms, &mut clips, &mut next_block_id);
                        deck.track_pos_sec = sec;
                        start_clip(deck, ev.elapsed_ms);
                    } else {
                        deck.track_pos_sec = sec;
                    }
                }
                if deck.clip_start_ms.is_none() && !deck.loop_active {
                    start_clip(deck, ev.elapsed_ms);
                }
            }

            "cue_preview_start" => {
                // Rust jumps the deck to the cue point; mirror it so the clip's
                // track_start_sec doesn't depend on earlier position side effects.
                if let Some(cp) = ev.cue_point_sec {
                    deck.track_pos_sec = cp;
                }
                if deck.clip_start_ms.is_none() && !deck.loop_active {
                    start_clip(deck, ev.elapsed_ms);
                }
            }

            "stop" | "stopped_at_cue" | "stop_at_cue" | "cue_set_and_stop" => {
                // cue_set_and_stop: user pressed CUE while playing, stops and
                // moves cue to current position.
                finalize_clip(deck, deck_id, ev.elapsed_ms, &mut clips, &mut next_block_id);
                if let Some(cp) = ev.cue_point_sec {
                    deck.track_pos_sec = cp;
                }
            }

            "cue_preview_end" => {
                finalize_clip(deck, deck_id, ev.elapsed_ms, &mut clips, &mut next_block_id);
                if let Some(cp) = ev.cue_point_sec {
                    deck.track_pos_sec = cp;
                }
            }

            // cue_move fires when the user presses CUE while stopped and away
            // from the cue point; the deck is not playing, so no clip to
            // finalize. Rust also clears any active loop region.
            "cue_move" => {
                deck.loop_start_sec = None;
                deck.loop_end_sec = None;
                deck.loop_active = false;
                deck.loop_engaged_ms = None;
                if let Some(cp) = ev.cue_point_sec {
                    deck.track_pos_sec = cp;
                }
            }

            "seek" => {
                if let Some(sec) = ev.sec {
                    if deck.clip_start_ms.is_some() && !deck.loop_active {
                        finalize_clip(deck, deck_id, ev.elapsed_ms, &mut clips, &mut next_block_id);
                        deck.track_pos_sec = sec;
                        start_clip(deck, ev.elapsed_ms);
                    } else {
                        deck.track_pos_sec = sec;
                    }
                }
            }

            "set_playback_rate" => {
                if let Some(rate) = ev.rate {
                    // Same floor as the sim and the engine.
                    deck.rate = rate.max(0.1);
                    record_eff_rate(deck, ev.elapsed_ms);
                }
            }

            "set_nudge" => {
                if let Some(percent) = ev.percent {
                    deck.nudge_factor = 1.0 + percent / 100.0;
                    record_eff_rate(deck, ev.elapsed_ms);
                }
            }

            "set_beat_grid" => {
                if let Some(bpm) = ev.bpm {
                    deck.bpm = Some(bpm);
                }
                if let Some(off) = ev.beat_offset_sec {
                    deck.beat_offset_sec = off;
                }
            }

            "loop_out" => {
                // start_sec is the loop start (= cue_point in Rust at the moment
                // loop_out fires).
                deck.loop_start_sec = ev.start_sec;
                deck.loop_end_sec = ev.end_sec;
                if deck.loop_start_sec.is_some() && deck.loop_end_sec.is_some() {
                    finalize_clip(deck, deck_id, ev.elapsed_ms, &mut clips, &mut next_block_id);
                    engage_loop(deck, ev.elapsed_ms);
                }
            }

            "loop_in" => {
                // loop_in always clears the loop region in Rust; if we were
                // looping, exit first.
                if deck.loop_active {
                    exit_loop_and_continue(
                        deck,
                        deck_id,
                        ev.elapsed_ms,
                        &mut clips,
                        &mut next_block_id,
                    );
                }
                deck.loop_start_sec = None;
                deck.loop_end_sec = None;
            }

            "exit_loop" => {
                // set_loop_active(false) also logs exit_loop, so both paths reach here.
                if deck.loop_active {
                    exit_loop_and_continue(
                        deck,
                        deck_id,
                        ev.elapsed_ms,
                        &mut clips,
                        &mut next_block_id,
                    );
                }
            }

            // The engine jumps the playhead back to the loop start on reloop.
            "reloop" if !deck.loop_active => {
                if let (Some(loop_start_sec), Some(_)) = (deck.loop_start_sec, deck.loop_end_sec) {
                    finalize_clip(deck, deck_id, ev.elapsed_ms, &mut clips, &mut next_block_id);
                    deck.track_pos_sec = loop_start_sec;
                    engage_loop(deck, ev.elapsed_ms);
                }
            }

            _ => {}
        }
    }

    let last_ms = events.last().map(|e| e.elapsed_ms).unwrap_or(0.0);
    for (deck_id, deck) in deck_states.iter_mut() {
        finalize_clip(deck, deck_id, last_ms, &mut clips, &mut next_block_id);
        finalize_loaded_span(deck, deck_id, last_ms, &mut loaded_spans);
    }

    ClipsBuild {
        clips,
        loaded_spans,
    }
}

pub const DEFAULT_GAIN: f64 = 1.0;
pub const DEFAULT_EQ_DB: f64 = 0.0;
pub const DEFAULT_FILTER_VALUE: f64 = 0.0;
pub const DEFAULT_RATE: f64 = 1.0;

// A lane narrower than this is unreadable, so a flat (or barely-pitched) session
// still draws at +/-8%. This is a timeline-drawing floor, not a pitch setting.
const MIN_RATE_RANGE_PCT: f64 = 8.0;

// The selectable lane ranges, derived from the caller's pitch-range options
// (the frontend's `PITCH_RANGE_OPTIONS`, passed across the WASM boundary so the
// values live in exactly one place). Options below the drawing floor are
// dropped; the smallest surviving step is the default range.
fn rate_steps_pct(pitch_options: &[f64]) -> Vec<f64> {
    let mut steps: Vec<f64> = pitch_options
        .iter()
        .copied()
        .filter(|&pct| pct >= MIN_RATE_RANGE_PCT)
        .collect();
    steps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if steps.is_empty() {
        steps.push(MIN_RATE_RANGE_PCT);
    }
    steps
}

// Smallest step that still covers the session's largest rate deviation.
pub fn rate_range_pct_for(max_deviation_pct: f64, steps: &[f64]) -> f64 {
    steps
        .iter()
        .copied()
        .find(|&pct| pct >= max_deviation_pct)
        .unwrap_or_else(|| steps.last().copied().unwrap_or(MIN_RATE_RANGE_PCT))
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LanePoint {
    pub ms: f64,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterActiveSpan {
    pub start_ms: f64,
    pub end_ms: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NudgeSpan {
    pub start_ms: f64,
    pub end_ms: f64,
    pub percent: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckLanes {
    pub gain: Vec<LanePoint>,
    pub eq_low: Vec<LanePoint>,
    pub eq_mid: Vec<LanePoint>,
    pub eq_high: Vec<LanePoint>,
    pub filter: Vec<LanePoint>,
    pub rate: Vec<LanePoint>,
    pub rate_min: f64,
    pub rate_max: f64,
    pub filter_active: Vec<FilterActiveSpan>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MasterLanes {
    pub gain: Vec<LanePoint>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanesBuild {
    pub deck_lanes: BTreeMap<String, DeckLanes>,
    pub master_lanes: MasterLanes,
    pub deck_nudges: BTreeMap<String, Vec<NudgeSpan>>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineBuild {
    pub clips: Vec<Clip>,
    pub loaded_spans: Vec<LoadedSpan>,
    pub deck_lanes: BTreeMap<String, DeckLanes>,
    pub master_lanes: MasterLanes,
    pub deck_nudges: BTreeMap<String, Vec<NudgeSpan>>,
}

// Clips and lanes from one event list. The editor needs both on every event
// change; deriving them together lets callers cross the WASM boundary once
// instead of serializing the event list twice.
pub fn build_timeline(
    events: &[SessionEvent],
    duration_ms: f64,
    pitch_options: &[f64],
) -> TimelineBuild {
    // build_clips assumes ordered input; playback sorts the same way.
    let mut sorted: Vec<SessionEvent> = events.to_vec();
    sorted.sort_by(crate::sim::event_sim_order);
    let clips = build_clips(&sorted);
    let lanes = build_lanes(&sorted, duration_ms, pitch_options);
    TimelineBuild {
        clips: clips.clips,
        loaded_spans: clips.loaded_spans,
        deck_lanes: lanes.deck_lanes,
        master_lanes: lanes.master_lanes,
        deck_nudges: lanes.deck_nudges,
    }
}

fn make_deck_lanes() -> DeckLanes {
    DeckLanes {
        gain: vec![LanePoint {
            ms: 0.0,
            value: DEFAULT_GAIN,
        }],
        eq_low: vec![LanePoint {
            ms: 0.0,
            value: DEFAULT_EQ_DB,
        }],
        eq_mid: vec![LanePoint {
            ms: 0.0,
            value: DEFAULT_EQ_DB,
        }],
        eq_high: vec![LanePoint {
            ms: 0.0,
            value: DEFAULT_EQ_DB,
        }],
        filter: vec![LanePoint {
            ms: 0.0,
            value: DEFAULT_FILTER_VALUE,
        }],
        rate: vec![LanePoint {
            ms: 0.0,
            value: DEFAULT_RATE,
        }],
        // Placeholder: build_lanes overwrites both once the session's largest
        // rate deviation (and thus the range step) is known.
        rate_min: DEFAULT_RATE,
        rate_max: DEFAULT_RATE,
        filter_active: Vec::new(),
    }
}

fn extend_to_end(points: &mut Vec<LanePoint>, duration_ms: f64) {
    if let Some(last) = points.last() {
        if last.ms < duration_ms {
            points.push(LanePoint {
                ms: duration_ms,
                value: last.value,
            });
        }
    }
}

pub fn build_lanes(events: &[SessionEvent], duration_ms: f64, pitch_options: &[f64]) -> LanesBuild {
    let mut deck_lanes: BTreeMap<String, DeckLanes> = BTreeMap::new();
    let mut filter_active_since_ms: BTreeMap<String, Option<f64>> = BTreeMap::new();
    let mut nudge_since: BTreeMap<String, Option<(f64, f64)>> = BTreeMap::new();
    let mut deck_nudges: BTreeMap<String, Vec<NudgeSpan>> = BTreeMap::new();
    let mut master_lanes = MasterLanes {
        gain: vec![LanePoint {
            ms: 0.0,
            value: DEFAULT_MASTER_GAIN as f64,
        }],
    };

    // Seed a deck's lanes (and span trackers) the first time any event names it.
    fn ensure_deck(
        id: &str,
        deck_lanes: &mut BTreeMap<String, DeckLanes>,
        filter_active_since_ms: &mut BTreeMap<String, Option<f64>>,
        nudge_since: &mut BTreeMap<String, Option<(f64, f64)>>,
        deck_nudges: &mut BTreeMap<String, Vec<NudgeSpan>>,
    ) {
        if !deck_lanes.contains_key(id) {
            deck_lanes.insert(id.to_string(), make_deck_lanes());
            filter_active_since_ms.insert(id.to_string(), None);
            nudge_since.insert(id.to_string(), None);
            deck_nudges.insert(id.to_string(), Vec::new());
        }
    }

    for ev in events {
        let deck_id = ev.deck.as_deref();
        match ev.event_type.as_str() {
            "set_volume" => {
                if let (Some(id), Some(gain)) = (deck_id, ev.gain) {
                    ensure_deck(
                        id,
                        &mut deck_lanes,
                        &mut filter_active_since_ms,
                        &mut nudge_since,
                        &mut deck_nudges,
                    );
                    deck_lanes.get_mut(id).unwrap().gain.push(LanePoint {
                        ms: ev.elapsed_ms,
                        value: gain as f64,
                    });
                }
            }

            "set_eq" => {
                if let (Some(id), Some(band), Some(db)) = (deck_id, ev.band.as_deref(), ev.db) {
                    ensure_deck(
                        id,
                        &mut deck_lanes,
                        &mut filter_active_since_ms,
                        &mut nudge_since,
                        &mut deck_nudges,
                    );
                    let auto = deck_lanes.get_mut(id).unwrap();
                    let lane = match band {
                        "low" => &mut auto.eq_low,
                        "mid" => &mut auto.eq_mid,
                        _ => &mut auto.eq_high,
                    };
                    lane.push(LanePoint {
                        ms: ev.elapsed_ms,
                        value: db as f64,
                    });
                }
            }

            "set_filter" => {
                if let (Some(id), Some(value)) = (deck_id, ev.value) {
                    ensure_deck(
                        id,
                        &mut deck_lanes,
                        &mut filter_active_since_ms,
                        &mut nudge_since,
                        &mut deck_nudges,
                    );
                    deck_lanes.get_mut(id).unwrap().filter.push(LanePoint {
                        ms: ev.elapsed_ms,
                        value: value as f64,
                    });
                }
            }

            "deck_snapshot" => {
                if let (Some(id), Some(rate)) = (deck_id, ev.playback_rate) {
                    ensure_deck(
                        id,
                        &mut deck_lanes,
                        &mut filter_active_since_ms,
                        &mut nudge_since,
                        &mut deck_nudges,
                    );
                    deck_lanes.get_mut(id).unwrap().rate.push(LanePoint {
                        ms: ev.elapsed_ms,
                        value: rate,
                    });
                }
            }

            "set_playback_rate" => {
                if let (Some(id), Some(rate)) = (deck_id, ev.rate) {
                    ensure_deck(
                        id,
                        &mut deck_lanes,
                        &mut filter_active_since_ms,
                        &mut nudge_since,
                        &mut deck_nudges,
                    );
                    deck_lanes.get_mut(id).unwrap().rate.push(LanePoint {
                        ms: ev.elapsed_ms,
                        value: rate,
                    });
                }
            }

            "set_filter_active" => {
                if let (Some(id), Some(active)) = (deck_id, ev.active) {
                    ensure_deck(
                        id,
                        &mut deck_lanes,
                        &mut filter_active_since_ms,
                        &mut nudge_since,
                        &mut deck_nudges,
                    );
                    let since = filter_active_since_ms.get_mut(id).unwrap();
                    if active && since.is_none() {
                        *since = Some(ev.elapsed_ms);
                    } else if !active {
                        if let Some(start) = *since {
                            deck_lanes
                                .get_mut(id)
                                .unwrap()
                                .filter_active
                                .push(FilterActiveSpan {
                                    start_ms: start,
                                    end_ms: ev.elapsed_ms,
                                });
                            *since = None;
                        }
                    }
                }
            }

            // A nudge interval runs from the first non-zero `percent` event to
            // the following `percent: 0` event for that deck (mirrors
            // filter-active pairing).
            "set_nudge" => {
                if let (Some(id), Some(percent)) = (deck_id, ev.percent) {
                    ensure_deck(
                        id,
                        &mut deck_lanes,
                        &mut filter_active_since_ms,
                        &mut nudge_since,
                        &mut deck_nudges,
                    );
                    let since = nudge_since.get_mut(id).unwrap();
                    if percent != 0.0 {
                        match since {
                            None => *since = Some((ev.elapsed_ms, percent)),
                            Some((_start, p)) => *p = percent,
                        }
                    } else if let Some((start, p)) = *since {
                        deck_nudges.get_mut(id).unwrap().push(NudgeSpan {
                            start_ms: start,
                            end_ms: ev.elapsed_ms,
                            percent: p,
                        });
                        *since = None;
                    }
                }
            }

            "set_master_gain" => {
                if let Some(gain) = ev.gain {
                    master_lanes.gain.push(LanePoint {
                        ms: ev.elapsed_ms,
                        value: gain as f64,
                    });
                }
            }

            _ => {}
        }
    }

    for (deck_id, auto) in deck_lanes.iter_mut() {
        if let Some(Some(since)) = filter_active_since_ms.get(deck_id) {
            auto.filter_active.push(FilterActiveSpan {
                start_ms: *since,
                end_ms: duration_ms,
            });
        }
        if let Some(Some((start, percent))) = nudge_since.get(deck_id) {
            deck_nudges.get_mut(deck_id).unwrap().push(NudgeSpan {
                start_ms: *start,
                end_ms: duration_ms,
                percent: *percent,
            });
        }
        extend_to_end(&mut auto.gain, duration_ms);
        extend_to_end(&mut auto.eq_low, duration_ms);
        extend_to_end(&mut auto.eq_mid, duration_ms);
        extend_to_end(&mut auto.eq_high, duration_ms);
        extend_to_end(&mut auto.filter, duration_ms);
        extend_to_end(&mut auto.rate, duration_ms);
    }
    extend_to_end(&mut master_lanes.gain, duration_ms);

    let mut max_rate_deviation_pct = 0.0f64;
    for auto in deck_lanes.values() {
        for p in &auto.rate {
            max_rate_deviation_pct = max_rate_deviation_pct.max((p.value - 1.0).abs() * 100.0);
        }
    }
    let range_pct = rate_range_pct_for(max_rate_deviation_pct, &rate_steps_pct(pitch_options));
    for auto in deck_lanes.values_mut() {
        auto.rate_min = 1.0 - range_pct / 100.0;
        auto.rate_max = 1.0 + range_pct / 100.0;
    }

    LanesBuild {
        deck_lanes,
        master_lanes,
        deck_nudges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The frontend's PITCH_RANGE_OPTIONS, the single source these tests pin
    // their expected lane ranges to (8% -> +/-0.08, 10% -> +/-0.10, etc.).
    const PITCH_OPTS: [f64; 6] = [6.0, 8.0, 10.0, 16.0, 50.0, 100.0];

    fn ev(event_type: &str, elapsed_ms: f64, deck: Option<&str>) -> SessionEvent {
        SessionEvent {
            event_type: event_type.to_string(),
            elapsed_ms,
            deck: deck.map(|d| d.to_string()),
            ..Default::default()
        }
    }

    // The WASM boundary is JSON-in/JSON-out; the TS wrapper expects camelCase
    // keys, including the top-level `loadedSpans` field.
    #[test]
    fn clips_build_serializes_loaded_spans_as_camel_case() {
        let json = serde_json::to_value(ClipsBuild {
            clips: vec![],
            loaded_spans: vec![],
        })
        .unwrap();
        assert!(json.get("loadedSpans").is_some());
        assert!(json.get("loaded_spans").is_none());
    }

    #[test]
    fn wave_segments_split_at_rate_and_nudge_and_stamp_grid() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            SessionEvent {
                bpm: Some(120.0),
                beat_offset_sec: Some(0.1),
                ..ev("set_beat_grid", 50.0, Some("A"))
            },
            ev("play", 1000.0, Some("A")),
            SessionEvent {
                rate: Some(2.0),
                ..ev("set_playback_rate", 3000.0, Some("A"))
            },
            SessionEvent {
                percent: Some(50.0),
                ..ev("set_nudge", 4000.0, Some("A"))
            },
            ev("stop", 5000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        assert_eq!(clips.len(), 1);
        let c = &clips[0];
        // Grid captured at clip start rides on the clip.
        assert_eq!(c.bpm, Some(120.0));
        assert_eq!(c.beat_offset_sec, Some(0.1));
        // Three constant-rate pieces: 1x, then 2x, then 2x*1.5=3x effective.
        let segs = &c.wave_segments;
        assert_eq!(segs.len(), 3);
        let approx = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(approx(segs[0].wall_start_ms, 1000.0) && approx(segs[0].wall_end_ms, 3000.0));
        assert!(approx(segs[0].track_start_sec, 0.0) && approx(segs[0].track_end_sec, 2.0));
        assert!(approx(segs[1].wall_start_ms, 3000.0) && approx(segs[1].wall_end_ms, 4000.0));
        assert!(approx(segs[1].track_start_sec, 2.0) && approx(segs[1].track_end_sec, 4.0));
        assert!(approx(segs[2].wall_start_ms, 4000.0) && approx(segs[2].wall_end_ms, 5000.0));
        assert!(approx(segs[2].track_start_sec, 4.0) && approx(segs[2].track_end_sec, 7.0));
        // Segments tile the clip with no gaps and cover the full track advance.
        assert!(approx(segs[0].track_end_sec, segs[1].track_start_sec));
        assert!(approx(segs[1].track_end_sec, segs[2].track_start_sec));
    }

    #[test]
    fn creates_a_clip_from_play_to_stop() {
        let events = vec![
            SessionEvent {
                path: Some("/tracks/song.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 1000.0, Some("A")),
            SessionEvent {
                cue_point_sec: Some(4.0),
                ..ev("stop", 5000.0, Some("A"))
            },
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        assert_eq!(clips.len(), 1);
        let c = &clips[0];
        assert_eq!(c.deck, "A");
        assert_eq!(c.session_start_ms, 1000.0);
        assert_eq!(c.session_end_ms, 5000.0);
        assert_eq!(c.track_path, "/tracks/song.mp3");
        assert_eq!(c.track_start_sec, 0.0);
        assert_eq!(c.playback_rate, 1.0);
    }

    #[test]
    fn ignores_play_events_when_clip_is_already_open() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 100.0, Some("A")),
            ev("play", 200.0, Some("A")),
            ev("stop", 500.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].session_start_ms, 100.0);
    }

    #[test]
    fn finalizes_clip_at_session_end_if_still_playing() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 500.0, Some("A")),
            ev("recording_stop", 3000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].session_end_ms, 3000.0);
    }

    #[test]
    fn play_with_sec_mid_clip_splits_like_seek() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 1000.0, Some("A")),
            SessionEvent {
                sec: Some(30.0),
                ..ev("play", 3000.0, Some("A"))
            },
            ev("stop", 5000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].session_start_ms, 1000.0);
        assert_eq!(clips[0].session_end_ms, 3000.0);
        assert_eq!(clips[0].track_start_sec, 0.0);
        assert_eq!(clips[1].session_start_ms, 3000.0);
        assert_eq!(clips[1].session_end_ms, 5000.0);
        assert_eq!(clips[1].track_start_sec, 30.0);
    }

    #[test]
    fn splits_clip_at_seek_and_starts_new_one() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 0.0, Some("A")),
            SessionEvent {
                sec: Some(10.0),
                ..ev("seek", 2000.0, Some("A"))
            },
            ev("stop", 4000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].session_start_ms, 0.0);
        assert_eq!(clips[0].session_end_ms, 2000.0);
        assert_eq!(clips[0].track_start_sec, 0.0);
        assert_eq!(clips[1].session_start_ms, 2000.0);
        assert_eq!(clips[1].session_end_ms, 4000.0);
        assert_eq!(clips[1].track_start_sec, 10.0);
    }

    #[test]
    fn renders_one_clip_per_loop_iteration() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 0.0, Some("A")),
            SessionEvent {
                sec: Some(4.0),
                ..ev("seek", 1000.0, Some("A"))
            },
            SessionEvent {
                start_sec: Some(4.0),
                end_sec: Some(6.0),
                ..ev("loop_out", 3000.0, Some("A"))
            },
            ev("exit_loop", 9000.0, Some("A")),
            ev("stop", 11000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        let loop_clips: Vec<&Clip> = clips.iter().filter(|c| c.track_start_sec == 4.0).collect();
        assert!(loop_clips.len() >= 3);
    }

    #[test]
    fn loop_clip_duration_matches_loop_region_duration() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 0.0, Some("A")),
            SessionEvent {
                sec: Some(1.0),
                ..ev("seek", 1000.0, Some("A"))
            },
            SessionEvent {
                start_sec: Some(2.0),
                end_sec: Some(4.0),
                ..ev("loop_out", 4000.0, Some("A"))
            },
            ev("exit_loop", 8000.0, Some("A")),
            ev("stop", 9000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        let loop_dur_ms = ((4.0 - 2.0) / 1.0) * 1000.0;
        let full = clips
            .iter()
            .filter(|c| c.track_start_sec == 2.0)
            .filter(|c| (c.session_end_ms - c.session_start_ms - loop_dur_ms).abs() < 1.0)
            .count();
        assert_eq!(full, 2);
    }

    #[test]
    fn partial_final_iteration_ends_at_exit_time() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 0.0, Some("A")),
            SessionEvent {
                start_sec: Some(0.0),
                end_sec: Some(2.0),
                ..ev("loop_out", 0.0, Some("A"))
            },
            ev("exit_loop", 5000.0, Some("A")),
            ev("stop", 6000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        let loop_clips: Vec<&Clip> = clips
            .iter()
            .filter(|c| c.session_start_ms < 5000.0)
            .collect();
        let last = loop_clips.last().unwrap();
        assert!(last.session_end_ms <= 5000.0);
    }

    #[test]
    fn loop_in_while_looping_exits_the_loop_and_continues() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 0.0, Some("A")),
            SessionEvent {
                start_sec: Some(1.0),
                end_sec: Some(3.0),
                ..ev("loop_out", 0.0, Some("A"))
            },
            ev("loop_in", 6000.0, Some("A")),
            ev("stop", 9000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        let post = clips.iter().find(|c| c.session_start_ms == 6000.0);
        assert!(post.is_some());
        assert_eq!(post.unwrap().session_end_ms, 9000.0);
    }

    #[test]
    fn reloop_re_enters_loop_region() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 0.0, Some("A")),
            SessionEvent {
                start_sec: Some(0.0),
                end_sec: Some(2.0),
                ..ev("loop_out", 0.0, Some("A"))
            },
            ev("exit_loop", 4000.0, Some("A")),
            ev("reloop", 5000.0, Some("A")),
            ev("exit_loop", 7000.0, Some("A")),
            ev("stop", 8000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        let reloop_clips = clips
            .iter()
            .filter(|c| {
                c.session_start_ms >= 5000.0
                    && c.session_start_ms < 7000.0
                    && c.track_start_sec == 0.0
            })
            .count();
        assert!(reloop_clips >= 1);
    }

    #[test]
    fn deck_snapshot_uses_cue_point_as_loop_start() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                position_sec: Some(4.0),
                cue_point_sec: Some(4.0),
                is_playing: Some(true),
                loop_active: Some(true),
                loop_end_sec: Some(6.0),
                playback_rate: Some(1.0),
                ..ev("deck_snapshot", 0.0, Some("A"))
            },
            ev("stop", 4000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        let loop_clips = clips.iter().filter(|c| c.track_start_sec == 4.0).count();
        assert!(loop_clips >= 2);
    }

    #[test]
    fn snapshot_without_loop_active_starts_a_regular_clip() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                position_sec: Some(10.0),
                cue_point_sec: Some(10.0),
                is_playing: Some(true),
                loop_active: Some(false),
                playback_rate: Some(1.0),
                ..ev("deck_snapshot", 0.0, Some("A"))
            },
            ev("stop", 2000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].session_start_ms, 0.0);
        assert_eq!(clips[0].session_end_ms, 2000.0);
        assert_eq!(clips[0].track_start_sec, 10.0);
    }

    #[test]
    fn finalizes_prior_clip_when_new_track_is_loaded() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 0.0, Some("A")),
            SessionEvent {
                path: Some("/t/b.mp3".to_string()),
                ..ev("load_track", 3000.0, Some("A"))
            },
            ev("play", 4000.0, Some("A")),
            ev("stop", 6000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].track_path, "/t/a.mp3");
        assert_eq!(clips[1].track_path, "/t/b.mp3");
    }

    #[test]
    fn loaded_span_covers_full_loaded_time() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("eject_track", 5000.0, Some("A")),
        ];
        let ClipsBuild { loaded_spans, .. } = build_clips(&events);
        assert_eq!(loaded_spans.len(), 1);
        assert_eq!(loaded_spans[0].start_ms, 0.0);
        assert_eq!(loaded_spans[0].end_ms, 5000.0);
    }

    #[test]
    fn deck_snapshot_initializes_loaded_span_at_time_zero() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                position_sec: Some(0.0),
                is_playing: Some(false),
                playback_rate: Some(1.0),
                ..ev("deck_snapshot", 0.0, Some("A"))
            },
            ev("eject_track", 3000.0, Some("A")),
        ];
        let ClipsBuild { loaded_spans, .. } = build_clips(&events);
        assert_eq!(loaded_spans.len(), 1);
        assert_eq!(loaded_spans[0].start_ms, 0.0);
    }

    #[test]
    fn bare_resume_play_continues_from_where_stop_left_the_deck() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 1000.0, Some("A")),
            ev("stop", 5000.0, Some("A")),
            ev("play", 6000.0, Some("A")),
            ev("stop", 8000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        assert_eq!(clips.len(), 2);
        assert!((clips[1].track_start_sec - 4.0).abs() < 1e-6);
    }

    #[test]
    fn integrates_rate_changes_within_the_playing_span() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 1000.0, Some("A")),
            SessionEvent {
                rate: Some(1.5),
                ..ev("set_playback_rate", 3000.0, Some("A"))
            },
            ev("stop", 5000.0, Some("A")),
            ev("play", 6000.0, Some("A")),
            ev("stop", 7000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        assert!((clips[1].track_start_sec - (2.0 + 2.0 * 1.5)).abs() < 1e-6);
    }

    #[test]
    fn integrates_nudges_within_the_playing_span() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 1000.0, Some("A")),
            SessionEvent {
                percent: Some(4.0),
                ..ev("set_nudge", 2000.0, Some("A"))
            },
            SessionEvent {
                percent: Some(0.0),
                ..ev("set_nudge", 3000.0, Some("A"))
            },
            ev("stop", 4000.0, Some("A")),
            ev("play", 5000.0, Some("A")),
            ev("stop", 6000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        assert!((clips[1].track_start_sec - 3.04).abs() < 1e-6);
    }

    #[test]
    fn load_track_resets_playback_rate_like_the_engine() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            SessionEvent {
                rate: Some(1.5),
                ..ev("set_playback_rate", 100.0, Some("A"))
            },
            ev("play", 1000.0, Some("A")),
            ev("stop", 2000.0, Some("A")),
            SessionEvent {
                path: Some("/t/b.mp3".to_string()),
                ..ev("load_track", 3000.0, Some("A"))
            },
            ev("play", 4000.0, Some("A")),
            ev("stop", 6000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        assert_eq!(clips.len(), 2);
        assert!((clips[1].playback_rate - 1.0).abs() < 1e-6);
        // 2s of wall time advance 2s of track at the reset rate, not 3s at the
        // stale 1.5.
        let seg = &clips[1].wave_segments[0];
        assert!((seg.track_end_sec - seg.track_start_sec - 2.0).abs() < 1e-6);
    }

    #[test]
    fn rate_changes_while_stopped_do_not_move_position() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 0.0, Some("A")),
            ev("stop", 2000.0, Some("A")),
            SessionEvent {
                rate: Some(1.5),
                ..ev("set_playback_rate", 3000.0, Some("A"))
            },
            ev("play", 4000.0, Some("A")),
            ev("stop", 5000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        assert!((clips[1].track_start_sec - 2.0).abs() < 1e-6);
        assert!((clips[1].playback_rate - 1.5).abs() < 1e-6);
    }

    #[test]
    fn play_latched_from_held_cue_preview_continues_from_preview_position() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            SessionEvent {
                cue_point_sec: Some(0.0),
                ..ev("cue_preview_start", 1000.0, Some("A"))
            },
            ev("play", 1500.0, Some("A")),
            ev("stop", 3000.0, Some("A")),
            ev("play", 4000.0, Some("A")),
            ev("stop", 5000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].track_start_sec, 0.0);
        assert!((clips[1].track_start_sec - 2.0).abs() < 1e-6);
    }

    #[test]
    fn released_cue_preview_returns_to_cue_point() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            SessionEvent {
                cue_point_sec: Some(5.0),
                ..ev("cue_preview_start", 1000.0, Some("A"))
            },
            SessionEvent {
                cue_point_sec: Some(5.0),
                ..ev("cue_preview_end", 2000.0, Some("A"))
            },
            ev("play", 3000.0, Some("A")),
            ev("stop", 4000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        let last = clips.last().unwrap();
        assert!((last.track_start_sec - 5.0).abs() < 1e-6);
    }

    #[test]
    fn loop_out_past_loop_end_wraps_playhead_into_loop() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 0.0, Some("A")),
            SessionEvent {
                start_sec: Some(0.0),
                end_sec: Some(6.0),
                ..ev("loop_out", 7000.0, Some("A"))
            },
            ev("stop", 8000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        let first = clips.iter().find(|c| c.session_start_ms == 7000.0).unwrap();
        assert!((first.track_start_sec - 1.0).abs() < 1e-6);
        assert_eq!(
            first.loop_region,
            Some(LoopRegion {
                start_sec: 0.0,
                end_sec: 6.0
            })
        );
    }

    #[test]
    fn resume_after_wrapped_loop_exit_reflects_entry_offset() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 0.0, Some("A")),
            SessionEvent {
                start_sec: Some(0.0),
                end_sec: Some(6.0),
                ..ev("loop_out", 7000.0, Some("A"))
            },
            ev("exit_loop", 9000.0, Some("A")),
            ev("stop", 10000.0, Some("A")),
            ev("play", 11000.0, Some("A")),
            ev("stop", 12000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        let last = clips.last().unwrap();
        assert_eq!(last.session_start_ms, 11000.0);
        // entry wrapped to 1.0, +2s in loop = 3.0 at exit, +1s to stop = 4.0
        assert!((last.track_start_sec - 4.0).abs() < 1e-6);
    }

    #[test]
    fn reloop_jumps_playhead_to_loop_start() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            ev("play", 0.0, Some("A")),
            SessionEvent {
                start_sec: Some(0.0),
                end_sec: Some(2.0),
                ..ev("loop_out", 3000.0, Some("A"))
            },
            ev("exit_loop", 3500.0, Some("A")),
            ev("reloop", 4000.0, Some("A")),
            ev("stop", 5000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        let relooped = clips.iter().find(|c| c.session_start_ms == 4000.0).unwrap();
        assert!((relooped.track_start_sec - 0.0).abs() < 1e-6);
    }

    #[test]
    fn snapshot_mid_loop_keeps_snapshot_position_for_first_iteration() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                position_sec: Some(5.0),
                cue_point_sec: Some(4.0),
                is_playing: Some(true),
                loop_active: Some(true),
                loop_end_sec: Some(6.0),
                playback_rate: Some(1.0),
                ..ev("deck_snapshot", 0.0, Some("A"))
            },
            ev("stop", 4000.0, Some("A")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        assert!((clips[0].track_start_sec - 5.0).abs() < 1e-6);
        assert!((clips[0].session_end_ms - clips[0].session_start_ms - 1000.0).abs() < 1.0);
        assert!((clips[1].track_start_sec - 4.0).abs() < 1e-6);
    }

    #[test]
    fn clips_from_different_decks_do_not_interfere() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev("load_track", 0.0, Some("A"))
            },
            SessionEvent {
                path: Some("/t/b.mp3".to_string()),
                ..ev("load_track", 0.0, Some("B"))
            },
            ev("play", 0.0, Some("A")),
            ev("play", 1000.0, Some("B")),
            ev("stop", 3000.0, Some("A")),
            ev("stop", 5000.0, Some("B")),
        ];
        let ClipsBuild { clips, .. } = build_clips(&events);
        let deck_a: Vec<&Clip> = clips.iter().filter(|c| c.deck == "A").collect();
        let deck_b: Vec<&Clip> = clips.iter().filter(|c| c.deck == "B").collect();
        assert_eq!(deck_a.len(), 1);
        assert_eq!(deck_b.len(), 1);
        assert_eq!(deck_a[0].session_end_ms, 3000.0);
        assert_eq!(deck_b[0].session_end_ms, 5000.0);
    }

    #[test]
    fn seeds_a_deck_with_default_lane_values() {
        let events = vec![SessionEvent {
            gain: Some(1.0),
            ..ev("set_volume", 0.0, Some("A"))
        }];
        let LanesBuild { deck_lanes, .. } = build_lanes(&events, 10_000.0, &PITCH_OPTS);
        let a = &deck_lanes["A"];
        assert_eq!(
            a.gain[0],
            LanePoint {
                ms: 0.0,
                value: 1.0
            }
        );
        assert_eq!(a.eq_low[0].value, 0.0);
        assert_eq!(a.eq_mid[0].value, 0.0);
        assert_eq!(a.eq_high[0].value, 0.0);
        assert_eq!(a.filter[0].value, 0.0);
        assert!(a.filter_active.is_empty());
    }

    #[test]
    fn seeds_master_gain_default_at_ms_zero() {
        let LanesBuild { master_lanes, .. } = build_lanes(&[], 10_000.0, &PITCH_OPTS);
        assert_eq!(master_lanes.gain[0].ms, 0.0);
    }

    #[test]
    fn extends_every_lane_to_session_end() {
        let events = vec![
            SessionEvent {
                gain: Some(1.0),
                ..ev("set_volume", 0.0, Some("A"))
            },
            SessionEvent {
                gain: Some(0.5),
                ..ev("set_volume", 2000.0, Some("A"))
            },
        ];
        let LanesBuild {
            deck_lanes,
            master_lanes,
            ..
        } = build_lanes(&events, 10_000.0, &PITCH_OPTS);
        let gain = &deck_lanes["A"].gain;
        assert_eq!(
            *gain.last().unwrap(),
            LanePoint {
                ms: 10_000.0,
                value: 0.5
            }
        );
        assert_eq!(master_lanes.gain.last().unwrap().ms, 10_000.0);
    }

    #[test]
    fn appends_gain_points_for_set_volume() {
        let events = vec![SessionEvent {
            gain: Some(0.7),
            ..ev("set_volume", 1000.0, Some("A"))
        }];
        let LanesBuild { deck_lanes, .. } = build_lanes(&events, 5000.0, &PITCH_OPTS);
        let gain = &deck_lanes["A"].gain;
        assert_eq!(gain.len(), 3);
        assert_eq!(
            gain[0],
            LanePoint {
                ms: 0.0,
                value: 1.0
            }
        );
        // gain is f32 in the event model (it drives the audio engine), so the
        // lane value carries f32 precision: 0.7f32 != 0.7f64 exactly.
        assert_eq!(gain[1].ms, 1000.0);
        assert!((gain[1].value - 0.7).abs() < 1e-6);
        assert_eq!(gain[2].ms, 5000.0);
        assert!((gain[2].value - 0.7).abs() < 1e-6);
    }

    #[test]
    fn routes_set_eq_points_to_band_lane() {
        let events = vec![
            SessionEvent {
                band: Some("low".to_string()),
                db: Some(-3.0),
                ..ev("set_eq", 100.0, Some("A"))
            },
            SessionEvent {
                band: Some("mid".to_string()),
                db: Some(2.0),
                ..ev("set_eq", 200.0, Some("A"))
            },
            SessionEvent {
                band: Some("high".to_string()),
                db: Some(4.0),
                ..ev("set_eq", 300.0, Some("A"))
            },
        ];
        let LanesBuild { deck_lanes, .. } = build_lanes(&events, 5000.0, &PITCH_OPTS);
        assert_eq!(deck_lanes["A"].eq_low[1].value, -3.0);
        assert_eq!(deck_lanes["A"].eq_mid[1].value, 2.0);
        assert_eq!(deck_lanes["A"].eq_high[1].value, 4.0);
    }

    #[test]
    fn appends_master_gain_points_regardless_of_deck() {
        let events = vec![SessionEvent {
            gain: Some(0.5),
            ..ev("set_master_gain", 1500.0, None)
        }];
        let LanesBuild { master_lanes, .. } = build_lanes(&events, 5000.0, &PITCH_OPTS);
        assert_eq!(master_lanes.gain[1].value, 0.5);
        assert_eq!(master_lanes.gain[1].ms, 1500.0);
        assert_eq!(master_lanes.gain.last().unwrap().ms, 5000.0);
    }

    #[test]
    fn pairs_filter_active_transition_into_span() {
        let events = vec![
            SessionEvent {
                active: Some(true),
                ..ev("set_filter_active", 1000.0, Some("A"))
            },
            SessionEvent {
                active: Some(false),
                ..ev("set_filter_active", 4000.0, Some("A"))
            },
        ];
        let LanesBuild { deck_lanes, .. } = build_lanes(&events, 10_000.0, &PITCH_OPTS);
        assert_eq!(
            deck_lanes["A"].filter_active,
            vec![FilterActiveSpan {
                start_ms: 1000.0,
                end_ms: 4000.0
            }]
        );
    }

    #[test]
    fn ignores_redundant_active_while_already_active() {
        let events = vec![
            SessionEvent {
                active: Some(true),
                ..ev("set_filter_active", 1000.0, Some("A"))
            },
            SessionEvent {
                active: Some(true),
                ..ev("set_filter_active", 2000.0, Some("A"))
            },
            SessionEvent {
                active: Some(false),
                ..ev("set_filter_active", 4000.0, Some("A"))
            },
        ];
        let LanesBuild { deck_lanes, .. } = build_lanes(&events, 10_000.0, &PITCH_OPTS);
        assert_eq!(
            deck_lanes["A"].filter_active,
            vec![FilterActiveSpan {
                start_ms: 1000.0,
                end_ms: 4000.0
            }]
        );
    }

    #[test]
    fn closes_unfinished_filter_span_at_session_end() {
        let events = vec![SessionEvent {
            active: Some(true),
            ..ev("set_filter_active", 7000.0, Some("A"))
        }];
        let LanesBuild { deck_lanes, .. } = build_lanes(&events, 10_000.0, &PITCH_OPTS);
        assert_eq!(
            deck_lanes["A"].filter_active,
            vec![FilterActiveSpan {
                start_ms: 7000.0,
                end_ms: 10_000.0
            }]
        );
    }

    #[test]
    fn pairs_nudge_percent_with_following_zero() {
        let events = vec![
            SessionEvent {
                percent: Some(8.0),
                ..ev("set_nudge", 1000.0, Some("A"))
            },
            SessionEvent {
                percent: Some(0.0),
                ..ev("set_nudge", 1500.0, Some("A"))
            },
        ];
        let LanesBuild { deck_nudges, .. } = build_lanes(&events, 10_000.0, &PITCH_OPTS);
        assert_eq!(
            deck_nudges["A"],
            vec![NudgeSpan {
                start_ms: 1000.0,
                end_ms: 1500.0,
                percent: 8.0
            }]
        );
    }

    #[test]
    fn keeps_original_start_when_percent_changes_mid_interval() {
        let events = vec![
            SessionEvent {
                percent: Some(4.0),
                ..ev("set_nudge", 1000.0, Some("A"))
            },
            SessionEvent {
                percent: Some(8.0),
                ..ev("set_nudge", 1200.0, Some("A"))
            },
            SessionEvent {
                percent: Some(0.0),
                ..ev("set_nudge", 1500.0, Some("A"))
            },
        ];
        let LanesBuild { deck_nudges, .. } = build_lanes(&events, 10_000.0, &PITCH_OPTS);
        assert_eq!(
            deck_nudges["A"],
            vec![NudgeSpan {
                start_ms: 1000.0,
                end_ms: 1500.0,
                percent: 8.0
            }]
        );
    }

    #[test]
    fn closes_unfinished_nudge_span_at_session_end() {
        let events = vec![SessionEvent {
            percent: Some(6.0),
            ..ev("set_nudge", 9000.0, Some("A"))
        }];
        let LanesBuild { deck_nudges, .. } = build_lanes(&events, 10_000.0, &PITCH_OPTS);
        assert_eq!(
            deck_nudges["A"],
            vec![NudgeSpan {
                start_ms: 9000.0,
                end_ms: 10_000.0,
                percent: 6.0
            }]
        );
    }

    #[test]
    fn keeps_nudge_spans_independent_across_decks() {
        let events = vec![
            SessionEvent {
                percent: Some(5.0),
                ..ev("set_nudge", 1000.0, Some("A"))
            },
            SessionEvent {
                percent: Some(-5.0),
                ..ev("set_nudge", 2000.0, Some("B"))
            },
            SessionEvent {
                percent: Some(0.0),
                ..ev("set_nudge", 3000.0, Some("A"))
            },
            SessionEvent {
                percent: Some(0.0),
                ..ev("set_nudge", 4000.0, Some("B"))
            },
        ];
        let LanesBuild { deck_nudges, .. } = build_lanes(&events, 10_000.0, &PITCH_OPTS);
        assert_eq!(
            deck_nudges["A"],
            vec![NudgeSpan {
                start_ms: 1000.0,
                end_ms: 3000.0,
                percent: 5.0
            }]
        );
        assert_eq!(
            deck_nudges["B"],
            vec![NudgeSpan {
                start_ms: 2000.0,
                end_ms: 4000.0,
                percent: -5.0
            }]
        );
    }

    #[test]
    fn rate_lane_seeds_at_one_and_appends_points() {
        let events = vec![SessionEvent {
            rate: Some(1.05),
            ..ev("set_playback_rate", 2000.0, Some("A"))
        }];
        let LanesBuild { deck_lanes, .. } = build_lanes(&events, 10_000.0, &PITCH_OPTS);
        assert_eq!(
            deck_lanes["A"].rate,
            vec![
                LanePoint {
                    ms: 0.0,
                    value: 1.0
                },
                LanePoint {
                    ms: 2000.0,
                    value: 1.05
                },
                LanePoint {
                    ms: 10_000.0,
                    value: 1.05
                },
            ]
        );
    }

    #[test]
    fn initial_rate_from_deck_snapshot_playback_rate() {
        let events = vec![SessionEvent {
            path: Some("/t/a.mp3".to_string()),
            position_sec: Some(0.0),
            is_playing: Some(false),
            playback_rate: Some(0.96),
            ..ev("deck_snapshot", 0.0, Some("A"))
        }];
        let LanesBuild { deck_lanes, .. } = build_lanes(&events, 5000.0, &PITCH_OPTS);
        assert_eq!(
            deck_lanes["A"].rate,
            vec![
                LanePoint {
                    ms: 0.0,
                    value: 1.0
                },
                LanePoint {
                    ms: 0.0,
                    value: 0.96
                },
                LanePoint {
                    ms: 5000.0,
                    value: 0.96
                },
            ]
        );
    }

    #[test]
    fn rate_range_from_largest_deviation_across_decks() {
        let events = vec![
            SessionEvent {
                rate: Some(1.09),
                ..ev("set_playback_rate", 100.0, Some("A"))
            },
            SessionEvent {
                gain: Some(0.5),
                ..ev("set_volume", 200.0, Some("B"))
            },
        ];
        let LanesBuild { deck_lanes, .. } = build_lanes(&events, 5000.0, &PITCH_OPTS);
        assert!((deck_lanes["A"].rate_min - 0.9).abs() < 1e-9);
        assert!((deck_lanes["A"].rate_max - 1.1).abs() < 1e-9);
        assert!((deck_lanes["B"].rate_min - 0.9).abs() < 1e-9);
        assert!((deck_lanes["B"].rate_max - 1.1).abs() < 1e-9);
    }

    #[test]
    fn rate_range_defaults_to_smallest_when_neutral() {
        let events = vec![SessionEvent {
            gain: Some(1.0),
            ..ev("set_volume", 0.0, Some("A"))
        }];
        let LanesBuild { deck_lanes, .. } = build_lanes(&events, 5000.0, &PITCH_OPTS);
        assert!((deck_lanes["A"].rate_min - 0.92).abs() < 1e-9);
        assert!((deck_lanes["A"].rate_max - 1.08).abs() < 1e-9);
    }

    #[test]
    fn rate_range_clamps_to_widest_step_for_extreme_rates() {
        let events = vec![SessionEvent {
            rate: Some(3.0),
            ..ev("set_playback_rate", 0.0, Some("A"))
        }];
        let LanesBuild { deck_lanes, .. } = build_lanes(&events, 5000.0, &PITCH_OPTS);
        assert!((deck_lanes["A"].rate_min - 0.0).abs() < 1e-9);
        assert!((deck_lanes["A"].rate_max - 2.0).abs() < 1e-9);
    }

    #[test]
    fn rate_range_pct_for_matches_steps() {
        assert_eq!(rate_range_pct_for(0.0, &rate_steps_pct(&PITCH_OPTS)), 8.0);
        assert_eq!(rate_range_pct_for(9.0, &rate_steps_pct(&PITCH_OPTS)), 10.0);
        assert_eq!(rate_range_pct_for(200.0, &rate_steps_pct(&PITCH_OPTS)), 100.0);
    }
}
