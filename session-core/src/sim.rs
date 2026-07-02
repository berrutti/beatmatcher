// Deterministic session simulation: replays the event stream into per-deck and
// per-strip state, and derives a deck's exact frame position at any time. This
// is the single source of truth shared by the live scheduler, the scrub
// snapshots, and (once wired) the frontend timeline via WASM.

use crate::event::{SessionCommand, SessionEvent};
use std::collections::HashMap;
use std::sync::Arc;

// -2 dBFS: gives the master bus headroom before the hardware clipping point.
// 10^(-2/20) = 0.7943...
pub const DEFAULT_MASTER_GAIN: f32 = 0.7943;

// Decoded track samples keyed by path. The simulation only reads each track's
// frame count (samples / channels); the buffers themselves are the live
// engine's, passed through unchanged.
pub type SampleCache = HashMap<String, (Arc<Vec<f32>>, usize)>;

// Internal simulation state. Not stored long-term.
#[derive(Clone)]
pub struct DeckSim {
    pub path: Option<String>,
    pub play_start_ms: f64,
    pub play_start_frame: f64,
    pub rate: f64,
    pub nudge_factor: f64,
    pub loop_active: bool,
    pub loop_start: f64,
    pub loop_end: f64,
    pub cue_point: f64,
    pub bpm: Option<f64>,
    pub beat_offset_frames: f64,
    pub total_frames: f64,
    pub is_playing: bool,
}

impl Default for DeckSim {
    fn default() -> Self {
        Self {
            path: None,
            play_start_ms: 0.0,
            play_start_frame: 0.0,
            rate: 1.0,
            nudge_factor: 1.0,
            loop_active: false,
            loop_start: 0.0,
            loop_end: 0.0,
            cue_point: 0.0,
            bpm: None,
            beat_offset_frames: 0.0,
            total_frames: 0.0,
            is_playing: false,
        }
    }
}

#[derive(Clone)]
pub struct StripSim {
    pub gain: f32,
    pub eq_low: f32,
    pub eq_mid: f32,
    pub eq_high: f32,
    pub filter_value: f32,
    pub filter_active: bool,
}

impl Default for StripSim {
    fn default() -> Self {
        Self {
            gain: 1.0,
            eq_low: 0.0,
            eq_mid: 0.0,
            eq_high: 0.0,
            filter_value: 0.0,
            filter_active: false,
        }
    }
}

#[derive(Default)]
pub struct SimState {
    pub decks: HashMap<String, DeckSim>,
    pub strips: HashMap<String, StripSim>,
    pub master_gain: f32,
}

impl SimState {
    pub fn new() -> Self {
        Self {
            decks: HashMap::new(),
            strips: HashMap::new(),
            master_gain: DEFAULT_MASTER_GAIN,
        }
    }
}

#[derive(Clone, Default)]
pub struct DeckSnap {
    pub path: Option<String>,
    pub position_frame: f64,
    pub is_playing: bool,
    pub rate: f64,
    pub nudge_factor: f64,
    pub loop_active: bool,
    pub loop_start: f64,
    pub loop_end: f64,
    pub cue_point: f64,
    pub bpm: Option<f64>,
    pub beat_offset_frames: f64,
    pub total_frames: f64,
}

#[derive(Clone)]
pub struct StripSnap {
    pub gain: f32,
    pub eq_low: f32,
    pub eq_mid: f32,
    pub eq_high: f32,
    pub filter_value: f32,
    pub filter_active: bool,
}

impl Default for StripSnap {
    fn default() -> Self {
        Self {
            gain: 1.0,
            eq_low: 0.0,
            eq_mid: 0.0,
            eq_high: 0.0,
            filter_value: 0.0,
            filter_active: false,
        }
    }
}

#[derive(Clone)]
pub struct SessionSnapshot {
    pub elapsed_ms: f64,
    pub decks: HashMap<String, DeckSnap>,
    pub strips: HashMap<String, StripSnap>,
    pub master_gain: f32,
}

// Continuous beat count at a playback position, given the track's beat grid.
// Consumers pick their own cycle length (4-beat phase ring, 16-beat phrase, ...)
// by taking this value modulo that length. Returns 0.0 for an unknown grid.
pub fn current_beat(position_sec: f64, beat_offset_sec: f64, bpm: f64) -> f64 {
    if bpm <= 0.0 {
        return 0.0;
    }
    (position_sec - beat_offset_sec) * bpm / 60.0
}

pub fn sim_pos(sim: &DeckSim, at_ms: f64, sr_f: f64) -> f64 {
    if !sim.is_playing {
        return sim.play_start_frame;
    }
    let effective_rate = sim.rate * sim.nudge_factor;
    let elapsed = (at_ms - sim.play_start_ms).max(0.0) / 1000.0 * sr_f * effective_rate;
    if sim.loop_active && sim.loop_end > sim.loop_start {
        let len = sim.loop_end - sim.loop_start;
        let offset = (sim.play_start_frame - sim.loop_start + elapsed).rem_euclid(len);
        (sim.loop_start + offset).clamp(0.0, sim.total_frames)
    } else {
        (sim.play_start_frame + elapsed).clamp(0.0, sim.total_frames)
    }
}

pub fn sim_state_from_snapshot(snap: &SessionSnapshot) -> SimState {
    let decks = snap
        .decks
        .iter()
        .map(|(id, d)| {
            (
                id.clone(),
                DeckSim {
                    path: d.path.clone(),
                    play_start_ms: snap.elapsed_ms,
                    play_start_frame: d.position_frame,
                    rate: d.rate,
                    nudge_factor: d.nudge_factor,
                    loop_active: d.loop_active,
                    loop_start: d.loop_start,
                    loop_end: d.loop_end,
                    cue_point: d.cue_point,
                    bpm: d.bpm,
                    beat_offset_frames: d.beat_offset_frames,
                    total_frames: d.total_frames,
                    is_playing: d.is_playing,
                },
            )
        })
        .collect();

    let strips = snap
        .strips
        .iter()
        .map(|(id, s)| {
            (
                id.clone(),
                StripSim {
                    gain: s.gain,
                    eq_low: s.eq_low,
                    eq_mid: s.eq_mid,
                    eq_high: s.eq_high,
                    filter_value: s.filter_value,
                    filter_active: s.filter_active,
                },
            )
        })
        .collect();

    SimState {
        decks,
        strips,
        master_gain: snap.master_gain,
    }
}

pub fn sim_apply_event(ev: &SessionEvent, state: &mut SimState, cache: &SampleCache, sr: u32) {
    let sr_f = sr as f64;
    let Some(cmd) = ev.command() else { return };

    match cmd {
        SessionCommand::DeckSnapshot {
            deck,
            path,
            position_sec,
            cue_point_sec,
            bpm,
            playback_rate,
            loop_active,
            loop_end_sec,
            is_playing,
        } => {
            let total_frames = cache
                .get(path)
                .map(|(s, c)| s.len() as f64 / *c as f64)
                .unwrap_or(0.0);
            let sim = state.decks.entry(deck.to_string()).or_default();
            sim.path = Some(path.to_string());
            sim.total_frames = total_frames;
            sim.rate = playback_rate.unwrap_or(1.0);
            let pos = position_sec.unwrap_or(0.0) * sr_f;
            sim.play_start_frame = pos;
            sim.play_start_ms = 0.0;
            sim.is_playing = is_playing;
            sim.loop_active = loop_active.unwrap_or(false);
            sim.loop_start = cue_point_sec.map_or(0.0, |c| c * sr_f);
            sim.cue_point = sim.loop_start;
            sim.loop_end = loop_end_sec.map_or(0.0, |e| e * sr_f);
            sim.bpm = bpm;
        }
        SessionCommand::LoadTrack {
            deck,
            path,
            beat_offset_sec,
        } => {
            let total_frames = cache
                .get(path)
                .map(|(s, c)| s.len() as f64 / *c as f64)
                .unwrap_or(0.0);
            let sim = state.decks.entry(deck.to_string()).or_default();
            sim.path = Some(path.to_string());
            sim.total_frames = total_frames;
            sim.rate = 1.0;
            let pos = beat_offset_sec.unwrap_or(0.0) * sr_f;
            sim.play_start_frame = pos;
            sim.play_start_ms = ev.elapsed_ms;
            sim.is_playing = false;
            sim.loop_active = false;
            // The live engine fully resets the deck on load: no loop region
            // or nudge survives into the new track.
            sim.loop_start = pos;
            sim.loop_end = 0.0;
            sim.nudge_factor = 1.0;
            sim.cue_point = pos;
            sim.bpm = None;
            sim.beat_offset_frames = pos;
        }
        SessionCommand::Play { deck, sec } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            sim.play_start_frame = sec
                .map(|s| s * sr_f)
                .unwrap_or_else(|| sim_pos(sim, ev.elapsed_ms, sr_f));
            sim.play_start_ms = ev.elapsed_ms;
            sim.is_playing = true;
        }
        SessionCommand::Stop { deck } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            sim.play_start_frame = sim_pos(sim, ev.elapsed_ms, sr_f);
            sim.play_start_ms = ev.elapsed_ms;
            sim.is_playing = false;
        }
        SessionCommand::StopAtCue {
            deck,
            cue_point_sec,
        } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            let pos = cue_point_sec
                .map(|c| c * sr_f)
                .unwrap_or_else(|| sim_pos(sim, ev.elapsed_ms, sr_f));
            sim.play_start_frame = pos;
            sim.play_start_ms = ev.elapsed_ms;
            sim.is_playing = false;
        }
        SessionCommand::Seek { deck, sec } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            sim.play_start_frame = sec * sr_f;
            sim.play_start_ms = ev.elapsed_ms;
            sim.loop_active =
                sim.loop_active && (sec * sr_f >= sim.loop_start) && (sec * sr_f < sim.loop_end);
        }
        SessionCommand::SetPlaybackRate { deck, rate } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            sim.play_start_frame = sim_pos(sim, ev.elapsed_ms, sr_f);
            sim.play_start_ms = ev.elapsed_ms;
            sim.rate = rate.max(0.1);
        }
        SessionCommand::SetNudge { deck, percent } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            sim.play_start_frame = sim_pos(sim, ev.elapsed_ms, sr_f);
            sim.play_start_ms = ev.elapsed_ms;
            sim.nudge_factor = 1.0 + percent / 100.0;
        }
        SessionCommand::LoopIn { deck, cue_sec } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            // Commit the current (possibly looped) position before clearing
            // loop_active, otherwise sim_pos would fall back to its raw linear
            // anchor and jump to where the deck would be had it never looped.
            sim.play_start_frame = sim_pos(sim, ev.elapsed_ms, sr_f);
            sim.play_start_ms = ev.elapsed_ms;
            if let Some(cs) = cue_sec {
                sim.loop_start = cs * sr_f;
                sim.cue_point = cs * sr_f;
            }
            sim.loop_end = 0.0;
            sim.loop_active = false;
        }
        SessionCommand::LoopOut {
            deck,
            start_sec,
            end_sec,
        } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            if let Some(ss) = start_sec {
                sim.loop_start = ss * sr_f;
                sim.cue_point = ss * sr_f;
            }
            if let Some(es) = end_sec {
                sim.loop_end = es * sr_f;
            }
            sim.loop_active = true;
        }
        SessionCommand::ExitLoop { deck } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            // Commit the current looped position before leaving the loop, so the
            // subsequent linear sim_pos continues from inside the loop region
            // instead of jumping to the un-looped linear position.
            sim.play_start_frame = sim_pos(sim, ev.elapsed_ms, sr_f);
            sim.play_start_ms = ev.elapsed_ms;
            sim.loop_active = false;
        }
        SessionCommand::Reloop { deck } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            if sim.loop_end > sim.loop_start {
                sim.play_start_frame = sim.loop_start;
                sim.play_start_ms = ev.elapsed_ms;
                if sim.is_playing {
                    sim.loop_active = true;
                }
            }
        }
        SessionCommand::EjectTrack { deck } => {
            state.decks.remove(deck);
        }
        SessionCommand::SetVolume { deck, gain } => {
            state.strips.entry(deck.to_string()).or_default().gain = gain;
        }
        SessionCommand::SetEq { deck, band, db } => {
            let strip = state.strips.entry(deck.to_string()).or_default();
            match band {
                "low" => strip.eq_low = db,
                "mid" => strip.eq_mid = db,
                "high" => strip.eq_high = db,
                _ => {}
            }
        }
        SessionCommand::SetFilter { deck, value } => {
            state
                .strips
                .entry(deck.to_string())
                .or_default()
                .filter_value = value;
        }
        SessionCommand::SetFilterActive { deck, active } => {
            state
                .strips
                .entry(deck.to_string())
                .or_default()
                .filter_active = active;
        }
        SessionCommand::SetMasterGain { gain } => {
            state.master_gain = gain;
        }
        SessionCommand::SetBeatGrid {
            deck,
            bpm,
            beat_offset_sec,
        } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            if let Some(bpm) = bpm {
                sim.bpm = Some(bpm);
            }
            if let Some(off) = beat_offset_sec {
                sim.beat_offset_frames = off * sr_f;
            }
        }
        SessionCommand::CuePreviewStart {
            deck,
            cue_point_sec,
        } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            let cp = cue_point_sec.map(|c| c * sr_f).unwrap_or(sim.cue_point);
            sim.cue_point = cp;
            sim.play_start_frame = cp;
            sim.play_start_ms = ev.elapsed_ms;
            sim.is_playing = true;
        }
        SessionCommand::CuePreviewEnd {
            deck,
            cue_point_sec,
        } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            let cp = cue_point_sec.map(|c| c * sr_f).unwrap_or(sim.cue_point);
            sim.play_start_frame = cp;
            sim.play_start_ms = ev.elapsed_ms;
            sim.is_playing = false;
        }
    }
}

fn snap_at(state: &SimState, at_ms: f64, sr_f: f64) -> SessionSnapshot {
    let decks = state
        .decks
        .iter()
        .map(|(id, sim)| {
            (
                id.clone(),
                DeckSnap {
                    path: sim.path.clone(),
                    position_frame: sim_pos(sim, at_ms, sr_f),
                    is_playing: sim.is_playing,
                    rate: sim.rate,
                    nudge_factor: sim.nudge_factor,
                    loop_active: sim.loop_active,
                    loop_start: sim.loop_start,
                    loop_end: sim.loop_end,
                    cue_point: sim.cue_point,
                    bpm: sim.bpm,
                    beat_offset_frames: sim.beat_offset_frames,
                    total_frames: sim.total_frames,
                },
            )
        })
        .collect();

    let strips = state
        .strips
        .iter()
        .map(|(id, s)| {
            (
                id.clone(),
                StripSnap {
                    gain: s.gain,
                    eq_low: s.eq_low,
                    eq_mid: s.eq_mid,
                    eq_high: s.eq_high,
                    filter_value: s.filter_value,
                    filter_active: s.filter_active,
                },
            )
        })
        .collect();

    SessionSnapshot {
        elapsed_ms: at_ms,
        decks,
        strips,
        master_gain: state.master_gain,
    }
}

// deck_snapshot (initial state) sorts first within its rounded-ms cluster; see
// the collapsed_load_play_at_start_reconstructs_playing test for why.
pub fn event_sim_order(a: &SessionEvent, b: &SessionEvent) -> std::cmp::Ordering {
    let bucket = |e: &SessionEvent| e.elapsed_ms.round() as i64;
    let snapshot_rank = |e: &SessionEvent| u8::from(e.event_type != "deck_snapshot");
    bucket(a)
        .cmp(&bucket(b))
        .then_with(|| snapshot_rank(a).cmp(&snapshot_rank(b)))
        .then_with(|| {
            a.elapsed_ms
                .partial_cmp(&b.elapsed_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

// Build one snapshot per event, capturing state AFTER the event fires.
// Scrubbing to time T finds the last snapshot with elapsed_ms <= T and loads it.
// The exact post-event state, so no event is ever missing or double-applied.
// Events are sorted before simulation so the state machine progresses correctly
// regardless of order in the source JSON.
pub fn build_snapshots(
    events: &[SessionEvent],
    sr: u32,
    cache: &SampleCache,
) -> Vec<SessionSnapshot> {
    let sr_f = sr as f64;
    let mut state = SimState::new();
    let mut snapshots = Vec::new();

    let mut sorted: Vec<&SessionEvent> = events.iter().collect();
    sorted.sort_by(|a, b| event_sim_order(a, b));

    // Snapshot at t=0 before any events (clean initial state).
    snapshots.push(snap_at(&state, 0.0, sr_f));

    for ev in sorted {
        sim_apply_event(ev, &mut state, cache, sr);
        // Snapshot AFTER the event: scrubbing to ev.elapsed_ms loads the
        // fully-applied post-event state, and the timer loop only replays
        // events strictly after from_ms.
        snapshots.push(snap_at(&state, ev.elapsed_ms, sr_f));
    }

    snapshots
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 44100;
    const SR_F: f64 = 44100.0;

    #[test]
    fn current_beat_counts_beats_from_offset() {
        assert!((current_beat(1.0, 0.0, 120.0) - 2.0).abs() < 1e-9);
        assert!((current_beat(2.5, 0.5, 120.0) - 4.0).abs() < 1e-9);
        assert_eq!(current_beat(10.0, 0.0, 0.0), 0.0);
    }

    fn deck_ev(event_type: &str, elapsed_ms: f64, deck: &str) -> SessionEvent {
        SessionEvent {
            event_type: event_type.to_string(),
            elapsed_ms,
            deck: Some(deck.to_string()),
            ..Default::default()
        }
    }

    fn cached_track(path: &str, seconds: usize) -> SampleCache {
        let mut cache: SampleCache = HashMap::new();
        cache.insert(
            path.to_string(),
            (Arc::new(vec![0.0f32; seconds * SR as usize * 2]), 2),
        );
        cache
    }

    fn playing_snapshot(path: &str) -> SessionEvent {
        SessionEvent {
            path: Some(path.to_string()),
            is_playing: Some(true),
            playback_rate: Some(1.0),
            position_sec: Some(0.0),
            cue_point_sec: Some(0.0),
            ..deck_ev("deck_snapshot", 0.0, "A")
        }
    }

    // ── scrub vs full-playthrough reconstruction parity ───────────────────────
    //
    // "Playing through to T" applies every event up to T then reads sim_pos(T).
    // "Scrubbing to T" is what start_session_playback does: find the nearest
    // snapshot, replay only the events between it and T, then read sim_pos(T).
    // The two MUST agree for every deck at every T, otherwise scrubbing lands a
    // deck at a different position than continuous playback would.

    fn playthrough_pos(
        events: &[SessionEvent],
        from_ms: f64,
        cache: &SampleCache,
        deck: &str,
    ) -> Option<f64> {
        let mut sorted: Vec<&SessionEvent> = events.iter().collect();
        sorted.sort_by(|a, b| a.elapsed_ms.partial_cmp(&b.elapsed_ms).unwrap());
        let mut state = SimState::new();
        for ev in sorted.iter().filter(|e| e.elapsed_ms <= from_ms) {
            sim_apply_event(ev, &mut state, cache, SR);
        }
        state.decks.get(deck).map(|d| sim_pos(d, from_ms, SR_F))
    }

    fn scrub_pos(
        events: &[SessionEvent],
        from_ms: f64,
        cache: &SampleCache,
        deck: &str,
    ) -> Option<f64> {
        let snaps = build_snapshots(events, SR, cache);
        let idx = snaps.partition_point(|s| s.elapsed_ms <= from_ms);
        let (mut sim, snapshot_ms) = match idx.checked_sub(1) {
            Some(i) => (sim_state_from_snapshot(&snaps[i]), snaps[i].elapsed_ms),
            None => (SimState::new(), 0.0),
        };
        let mut sorted: Vec<&SessionEvent> = events.iter().collect();
        sorted.sort_by(|a, b| a.elapsed_ms.partial_cmp(&b.elapsed_ms).unwrap());
        for ev in sorted
            .iter()
            .filter(|e| e.elapsed_ms > snapshot_ms && e.elapsed_ms <= from_ms)
        {
            sim_apply_event(ev, &mut sim, cache, SR);
        }
        sim.decks.get(deck).map(|d| sim_pos(d, from_ms, SR_F))
    }

    fn assert_scrub_matches_playthrough(
        events: &[SessionEvent],
        cache: &SampleCache,
        steps: usize,
        deck: &str,
    ) {
        for step in 0..=steps {
            let at_ms = step as f64 * 100.0;
            let playthrough = playthrough_pos(events, at_ms, cache, deck);
            let scrub = scrub_pos(events, at_ms, cache, deck);
            match (playthrough, scrub) {
                (Some(playthrough_frame), Some(scrub_frame)) => assert!(
                    (playthrough_frame - scrub_frame).abs() < 1.0,
                    "t={at_ms}ms playthrough={playthrough_frame} scrub={scrub_frame} diff={}",
                    playthrough_frame - scrub_frame
                ),
                (None, None) => {}
                _ => panic!("t={at_ms}ms one path produced a deck and the other didn't"),
            }
        }
    }

    #[test]
    fn scrub_matches_playthrough_for_realistic_session() {
        let path = "/fake/a.wav".to_string();
        let mut cache: SampleCache = HashMap::new();
        cache.insert(
            path.clone(),
            (Arc::new(vec![0.0f32; 60 * SR as usize * 2]), 2),
        );

        let mk = |t: f64, ty: &str, deck: &str| SessionEvent {
            event_type: ty.to_string(),
            elapsed_ms: t,
            deck: Some(deck.to_string()),
            ..Default::default()
        };

        let events = vec![
            SessionEvent {
                path: Some(path.clone()),
                is_playing: Some(true),
                playback_rate: Some(1.0),
                position_sec: Some(0.0),
                ..mk(0.0, "deck_snapshot", "A")
            },
            SessionEvent {
                rate: Some(1.03),
                ..mk(4000.0, "set_playback_rate", "A")
            },
            SessionEvent {
                start_sec: Some(8.0),
                end_sec: Some(10.0),
                ..mk(10_000.0, "loop_out", "A")
            },
            mk(18_000.0, "exit_loop", "A"),
            SessionEvent {
                sec: Some(20.0),
                ..mk(20_000.0, "seek", "A")
            },
        ];

        for step in 0..=300 {
            let t = step as f64 * 100.0; // every 100ms up to 30s
            let pt = playthrough_pos(&events, t, &cache, "A");
            let sc = scrub_pos(&events, t, &cache, "A");
            match (pt, sc) {
                (Some(p), Some(s)) => assert!(
                    (p - s).abs() < 1.0,
                    "t={t}ms playthrough={p} scrub={s} diff={}",
                    p - s
                ),
                (None, None) => {}
                _ => panic!("t={t}ms one path produced a deck and the other didn't"),
            }
        }
    }

    // Reconstructs deck state at from_ms exactly as start_session_playback does
    // (nearest snapshot, then replay the events up to from_ms), returning what
    // the live engine would set the deck to: whether it plays and which track.
    fn reconstruct_deck(
        events: &[SessionEvent],
        from_ms: f64,
        cache: &SampleCache,
        deck: &str,
    ) -> (bool, Option<String>) {
        let snaps = build_snapshots(events, SR, cache);
        let idx = snaps.partition_point(|s| s.elapsed_ms <= from_ms);
        let (mut sim, snapshot_ms) = match idx.checked_sub(1) {
            Some(i) => (sim_state_from_snapshot(&snaps[i]), snaps[i].elapsed_ms),
            None => (SimState::new(), 0.0),
        };
        let mut sorted: Vec<&SessionEvent> = events.iter().collect();
        sorted.sort_by(|a, b| a.elapsed_ms.partial_cmp(&b.elapsed_ms).unwrap());
        for ev in sorted.iter().filter(|e| {
            e.elapsed_ms > snapshot_ms && e.elapsed_ms <= from_ms && e.event_type != "deck_snapshot"
        }) {
            sim_apply_event(ev, &mut sim, cache, SR);
        }
        sim.decks
            .get(deck)
            .map(|d| (d.is_playing, d.path.clone()))
            .unwrap_or((false, None))
    }

    // A clip dragged to the session start collapses its load onto t=0, sharing
    // that millisecond with the deck_snapshot of a different (unplayed) track.
    // The live engine reconstructs from the LAST snapshot at/before t=0, so the
    // event order at t=0 decides the outcome: load_track forces is_playing=false,
    // play sets it true. If play does not end up last, the deck reconstructs as
    // "loaded but stopped" and live playback is silent even though the clip
    // renders. This guards the ordering contract the editor must honour.
    #[test]
    fn collapsed_load_play_at_start_reconstructs_playing() {
        let track1 = "/fake/track1.wav".to_string();
        let track2 = "/fake/track2.wav".to_string();
        let mut cache: SampleCache = HashMap::new();
        cache.insert(
            track1.clone(),
            (Arc::new(vec![0.0f32; 60 * SR as usize * 2]), 2),
        );
        cache.insert(
            track2.clone(),
            (Arc::new(vec![0.0f32; 60 * SR as usize * 2]), 2),
        );

        // The real failing order from a saved session: the deck_snapshot of the
        // unplayed track lands a sub-millisecond hair AFTER the dragged track's
        // play (float drift), so a plain timestamp sort orders the snapshot last.
        let play_ms = 0.025207999999992126;
        let snap_ms = 0.025208;
        let events = vec![
            SessionEvent {
                path: Some(track2.clone()),
                beat_offset_sec: Some(0.0),
                ..deck_ev("load_track", play_ms, "A")
            },
            SessionEvent {
                sec: Some(0.0),
                ..deck_ev("play", play_ms, "A")
            },
            SessionEvent {
                path: Some(track1.clone()),
                is_playing: Some(false),
                playback_rate: Some(1.0),
                position_sec: Some(0.0),
                ..deck_ev("deck_snapshot", snap_ms, "A")
            },
            deck_ev("stop", 8000.0, "A"),
        ];

        let (playing, path) = reconstruct_deck(&events, 1000.0, &cache, "A");
        assert!(
            playing,
            "deck A must reconstruct as playing inside the clip"
        );
        assert_eq!(path.as_deref(), Some(track2.as_str()));
    }

    // ── sim_pos ──────────────────────────────────────────────────────────────

    #[test]
    fn sim_pos_stopped_returns_start_frame() {
        let sim = DeckSim {
            is_playing: false,
            play_start_frame: 5000.0,
            ..Default::default()
        };
        assert_eq!(sim_pos(&sim, 99999.0, SR_F), 5000.0);
    }

    #[test]
    fn sim_pos_playing_advances_linearly() {
        let sim = DeckSim {
            is_playing: true,
            play_start_ms: 0.0,
            play_start_frame: 0.0,
            rate: 1.0,
            total_frames: 1_000_000.0,
            ..Default::default()
        };
        assert_eq!(sim_pos(&sim, 1000.0, SR_F), SR_F);
    }

    #[test]
    fn sim_pos_respects_playback_rate() {
        let sim = DeckSim {
            is_playing: true,
            play_start_ms: 0.0,
            play_start_frame: 0.0,
            rate: 2.0,
            total_frames: 1_000_000.0,
            ..Default::default()
        };
        assert_eq!(sim_pos(&sim, 1000.0, SR_F), SR_F * 2.0);
    }

    #[test]
    fn sim_pos_advances_from_nonzero_start_time() {
        let sim = DeckSim {
            is_playing: true,
            play_start_ms: 500.0,
            play_start_frame: SR_F / 2.0,
            rate: 1.0,
            total_frames: 1_000_000.0,
            ..Default::default()
        };
        assert_eq!(sim_pos(&sim, 1000.0, SR_F), SR_F);
    }

    #[test]
    fn sim_pos_clamps_to_total_frames() {
        let sim = DeckSim {
            is_playing: true,
            play_start_ms: 0.0,
            play_start_frame: 0.0,
            rate: 1.0,
            total_frames: 100.0,
            ..Default::default()
        };
        assert_eq!(sim_pos(&sim, 10000.0, SR_F), 100.0);
    }

    #[test]
    fn sim_pos_loop_wraps_at_boundary() {
        let sim = DeckSim {
            is_playing: true,
            play_start_ms: 0.0,
            play_start_frame: 0.0,
            rate: 1.0,
            loop_active: true,
            loop_start: 0.0,
            loop_end: SR_F,
            total_frames: 1_000_000.0,
            ..Default::default()
        };
        let pos = sim_pos(&sim, 1500.0, SR_F);
        assert!((pos - SR_F / 2.0).abs() < 1.0, "expected ~22050, got {pos}");
    }

    // ── sim_apply_event ───────────────────────────────────────────────────────

    #[test]
    fn apply_play_marks_deck_playing() {
        let mut state = SimState::new();
        sim_apply_event(
            &SessionEvent {
                event_type: "play".to_string(),
                elapsed_ms: 1000.0,
                deck: Some("A".to_string()),
                ..Default::default()
            },
            &mut state,
            &HashMap::new(),
            SR,
        );
        assert!(state.decks["A"].is_playing);
        assert_eq!(state.decks["A"].play_start_ms, 1000.0);
    }

    #[test]
    fn apply_stop_freezes_position_at_current_frame() {
        let mut state = SimState::new();
        state.decks.insert(
            "A".to_string(),
            DeckSim {
                is_playing: true,
                play_start_ms: 0.0,
                play_start_frame: 0.0,
                rate: 1.0,
                total_frames: 1_000_000.0,
                ..Default::default()
            },
        );
        sim_apply_event(
            &deck_ev("stop", 2000.0, "A"),
            &mut state,
            &HashMap::new(),
            SR,
        );
        assert!(!state.decks["A"].is_playing);
        // 2s × 44100 Hz = 88200 frames.
        assert_eq!(state.decks["A"].play_start_frame, 88200.0);
    }

    #[test]
    fn apply_seek_teleports_position() {
        let mut state = SimState::new();
        state.decks.insert(
            "A".to_string(),
            DeckSim {
                is_playing: true,
                ..Default::default()
            },
        );
        sim_apply_event(
            &SessionEvent {
                event_type: "seek".to_string(),
                elapsed_ms: 5000.0,
                deck: Some("A".to_string()),
                sec: Some(10.0),
                ..Default::default()
            },
            &mut state,
            &HashMap::new(),
            SR,
        );
        assert_eq!(state.decks["A"].play_start_frame, 10.0 * SR_F);
        assert_eq!(state.decks["A"].play_start_ms, 5000.0);
    }

    #[test]
    fn apply_rate_change_recomputes_position_at_change_time() {
        let mut state = SimState::new();
        state.decks.insert(
            "A".to_string(),
            DeckSim {
                is_playing: true,
                play_start_ms: 0.0,
                play_start_frame: 0.0,
                rate: 1.0,
                total_frames: 1_000_000.0,
                ..Default::default()
            },
        );
        sim_apply_event(
            &SessionEvent {
                event_type: "set_playback_rate".to_string(),
                elapsed_ms: 1000.0,
                deck: Some("A".to_string()),
                rate: Some(2.0),
                ..Default::default()
            },
            &mut state,
            &HashMap::new(),
            SR,
        );
        assert_eq!(state.decks["A"].play_start_frame, SR_F);
        assert_eq!(state.decks["A"].rate, 2.0);
        // At t=2000ms: 44100 + 44100×2 = 132300.
        assert_eq!(sim_pos(&state.decks["A"], 2000.0, SR_F), 132300.0);
    }

    #[test]
    fn apply_set_volume_updates_strip_gain() {
        let mut state = SimState::new();
        sim_apply_event(
            &SessionEvent {
                event_type: "set_volume".to_string(),
                deck: Some("A".to_string()),
                gain: Some(0.5),
                ..Default::default()
            },
            &mut state,
            &HashMap::new(),
            SR,
        );
        assert_eq!(state.strips["A"].gain, 0.5);
    }

    #[test]
    fn apply_set_eq_updates_all_three_bands() {
        let mut state = SimState::new();
        for (band, db) in [("low", -3.0f32), ("mid", -6.0), ("high", -9.0)] {
            sim_apply_event(
                &SessionEvent {
                    event_type: "set_eq".to_string(),
                    deck: Some("A".to_string()),
                    band: Some(band.to_string()),
                    db: Some(db),
                    ..Default::default()
                },
                &mut state,
                &HashMap::new(),
                SR,
            );
        }
        assert_eq!(state.strips["A"].eq_low, -3.0);
        assert_eq!(state.strips["A"].eq_mid, -6.0);
        assert_eq!(state.strips["A"].eq_high, -9.0);
    }

    #[test]
    fn apply_set_master_gain() {
        let mut state = SimState::new();
        sim_apply_event(
            &SessionEvent {
                event_type: "set_master_gain".to_string(),
                gain: Some(0.5),
                ..Default::default()
            },
            &mut state,
            &HashMap::new(),
            SR,
        );
        assert_eq!(state.master_gain, 0.5);
    }

    // ── build_snapshots ───────────────────────────────────────────────────────

    #[test]
    fn empty_session_produces_single_clean_snapshot() {
        let snaps = build_snapshots(&[], SR, &HashMap::new());
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].elapsed_ms, 0.0);
        assert!(snaps[0].decks.is_empty());
        assert_eq!(snaps[0].master_gain, DEFAULT_MASTER_GAIN);
    }

    #[test]
    fn snapshot_after_play_captures_playing_state() {
        let events = vec![SessionEvent {
            event_type: "play".to_string(),
            elapsed_ms: 1000.0,
            deck: Some("A".to_string()),
            ..Default::default()
        }];
        let snaps = build_snapshots(&events, SR, &HashMap::new());
        let last = snaps.last().unwrap();
        assert_eq!(last.elapsed_ms, 1000.0);
        assert!(last.decks.get("A").map(|d| d.is_playing).unwrap_or(false));
    }

    #[test]
    fn snapshot_at_t0_reflects_clean_state_before_play() {
        let events = vec![SessionEvent {
            event_type: "play".to_string(),
            elapsed_ms: 1000.0,
            deck: Some("A".to_string()),
            ..Default::default()
        }];
        let snaps = build_snapshots(&events, SR, &HashMap::new());
        assert!(!snaps[0]
            .decks
            .get("A")
            .map(|d| d.is_playing)
            .unwrap_or(false));
    }

    #[test]
    fn snapshots_sorted_even_with_unsorted_input_events() {
        let events = vec![
            SessionEvent {
                event_type: "play".to_string(),
                elapsed_ms: 2000.0,
                deck: Some("A".to_string()),
                ..Default::default()
            },
            SessionEvent {
                event_type: "stop".to_string(),
                elapsed_ms: 1000.0,
                deck: Some("A".to_string()),
                ..Default::default()
            },
        ];
        let snaps = build_snapshots(&events, SR, &HashMap::new());
        for i in 1..snaps.len() {
            assert!(
                snaps[i].elapsed_ms >= snaps[i - 1].elapsed_ms,
                "snapshot[{i}] ({}) < snapshot[{}] ({})",
                snaps[i].elapsed_ms,
                i - 1,
                snaps[i - 1].elapsed_ms,
            );
        }
        // Correct order: stop at 1000ms, play at 2000ms → last has is_playing=true.
        assert!(snaps
            .last()
            .unwrap()
            .decks
            .get("A")
            .map(|d| d.is_playing)
            .unwrap_or(false));
    }

    #[test]
    fn eq_change_captured_in_post_event_snapshot() {
        let events = vec![SessionEvent {
            event_type: "set_eq".to_string(),
            elapsed_ms: 3000.0,
            deck: Some("A".to_string()),
            band: Some("low".to_string()),
            db: Some(-6.0),
            ..Default::default()
        }];
        let snaps = build_snapshots(&events, SR, &HashMap::new());
        assert_eq!(
            snaps[0].strips.get("A").map(|s| s.eq_low).unwrap_or(0.0),
            0.0
        );
        assert_eq!(
            snaps
                .last()
                .unwrap()
                .strips
                .get("A")
                .map(|s| s.eq_low)
                .unwrap_or(0.0),
            -6.0
        );
    }

    // ── snapshot lookup (partition_point) ─────────────────────────────────────

    #[test]
    fn scrub_before_first_event_loads_clean_state() {
        let events = vec![SessionEvent {
            event_type: "play".to_string(),
            elapsed_ms: 5000.0,
            deck: Some("A".to_string()),
            ..Default::default()
        }];
        let snaps = build_snapshots(&events, SR, &HashMap::new());
        let idx = snaps.partition_point(|s| s.elapsed_ms <= 1000.0);
        let snap = &snaps[idx - 1];
        assert!(!snap.decks.get("A").map(|d| d.is_playing).unwrap_or(false));
    }

    #[test]
    fn scrub_between_events_sees_intermediate_state() {
        let events = vec![
            SessionEvent {
                event_type: "play".to_string(),
                elapsed_ms: 1000.0,
                deck: Some("A".to_string()),
                ..Default::default()
            },
            SessionEvent {
                event_type: "set_eq".to_string(),
                elapsed_ms: 5000.0,
                deck: Some("A".to_string()),
                band: Some("low".to_string()),
                db: Some(-6.0),
                ..Default::default()
            },
        ];
        let snaps = build_snapshots(&events, SR, &HashMap::new());
        // At 3000ms: after play, before EQ change.
        let idx = snaps.partition_point(|s| s.elapsed_ms <= 3000.0);
        let snap = &snaps[idx - 1];
        assert_eq!(snap.elapsed_ms, 1000.0);
        assert!(snap.decks.get("A").map(|d| d.is_playing).unwrap_or(false));
        assert_eq!(snap.strips.get("A").map(|s| s.eq_low).unwrap_or(0.0), 0.0);
    }

    #[test]
    fn two_deck_session_both_playing_after_second_play() {
        let events = vec![
            SessionEvent {
                event_type: "play".to_string(),
                elapsed_ms: 1000.0,
                deck: Some("A".to_string()),
                ..Default::default()
            },
            SessionEvent {
                event_type: "play".to_string(),
                elapsed_ms: 3000.0,
                deck: Some("B".to_string()),
                ..Default::default()
            },
        ];
        let snaps = build_snapshots(&events, SR, &HashMap::new());
        let idx = snaps.partition_point(|s| s.elapsed_ms <= 5000.0);
        let snap = &snaps[idx - 1];
        assert!(
            snap.decks.get("A").map(|d| d.is_playing).unwrap_or(false),
            "A not playing"
        );
        assert!(
            snap.decks.get("B").map(|d| d.is_playing).unwrap_or(false),
            "B not playing"
        );
    }

    // ── sim_state_from_snapshot round-trip ────────────────────────────────────

    #[test]
    fn round_trip_preserves_playing_deck_position() {
        let path = "/fake/track.mp3".to_string();
        let mut cache: SampleCache = HashMap::new();
        // 10 seconds of silence at 44100 Hz, mono. Enough room to advance 2s.
        cache.insert(path.clone(), (Arc::new(vec![0.0f32; 441_000]), 1));

        let events = vec![SessionEvent {
            event_type: "deck_snapshot".to_string(),
            elapsed_ms: 0.0,
            deck: Some("A".to_string()),
            path: Some(path),
            is_playing: Some(true),
            playback_rate: Some(1.0),
            position_sec: Some(0.0),
            ..Default::default()
        }];
        let snaps = build_snapshots(&events, SR, &cache);
        let snap = snaps.last().unwrap();
        let sim = sim_state_from_snapshot(snap);
        // 2s of play at rate=1 from frame 0 → 88200 (well within 441000).
        assert_eq!(sim_pos(&sim.decks["A"], 2000.0, SR_F), 88200.0);
    }

    // ── nudge tracking ────────────────────────────────────────────────────────

    #[test]
    fn sim_pos_accounts_for_nudge_factor() {
        // Nudge at +4% means the deck advances 4% faster.
        let sim = DeckSim {
            is_playing: true,
            play_start_ms: 0.0,
            play_start_frame: 0.0,
            rate: 1.0,
            nudge_factor: 1.04,
            total_frames: 1_000_000.0,
            ..Default::default()
        };
        let pos = sim_pos(&sim, 1000.0, SR_F);
        assert_eq!(pos, SR_F * 1.04);
    }

    #[test]
    fn sim_pos_unit_nudge_unchanged() {
        // nudge_factor = 1.0 must not change the result vs the non-nudge tests.
        let sim = DeckSim {
            is_playing: true,
            play_start_ms: 0.0,
            play_start_frame: 0.0,
            rate: 1.0,
            nudge_factor: 1.0,
            total_frames: 1_000_000.0,
            ..Default::default()
        };
        assert_eq!(sim_pos(&sim, 1000.0, SR_F), SR_F);
    }

    #[test]
    fn apply_nudge_commits_position_before_changing_factor() {
        // Same logic as the rate-change test: position must be locked at the
        // event time so the new factor only affects subsequent play.
        let mut state = SimState::new();
        state.decks.insert(
            "A".to_string(),
            DeckSim {
                is_playing: true,
                play_start_ms: 0.0,
                play_start_frame: 0.0,
                rate: 1.0,
                nudge_factor: 1.0,
                total_frames: 1_000_000.0,
                ..Default::default()
            },
        );
        // Apply +4% nudge at t=1000ms; position at that moment = 44100.
        sim_apply_event(
            &SessionEvent {
                event_type: "set_nudge".to_string(),
                elapsed_ms: 1000.0,
                deck: Some("A".to_string()),
                percent: Some(4.0),
                ..Default::default()
            },
            &mut state,
            &HashMap::new(),
            SR,
        );
        assert_eq!(state.decks["A"].play_start_frame, SR_F);
        assert_eq!(state.decks["A"].play_start_ms, 1000.0);
        assert!((state.decks["A"].nudge_factor - 1.04).abs() < 1e-9);

        // At t=2000ms: 44100 + 44100 * 1.04 = 44100 + 45864 = 89964.
        let pos = sim_pos(&state.decks["A"], 2000.0, SR_F);
        assert_eq!(pos, SR_F + SR_F * 1.04);
    }

    #[test]
    fn nudge_release_to_zero_commits_position() {
        let mut state = SimState::new();
        state.decks.insert(
            "A".to_string(),
            DeckSim {
                is_playing: true,
                play_start_ms: 0.0,
                play_start_frame: 0.0,
                rate: 1.0,
                nudge_factor: 1.04, // nudge already active
                total_frames: 1_000_000.0,
                ..Default::default()
            },
        );
        // Release nudge at t=1000ms; position = 1000ms * 1.04 * sr.
        sim_apply_event(
            &SessionEvent {
                event_type: "set_nudge".to_string(),
                elapsed_ms: 1000.0,
                deck: Some("A".to_string()),
                percent: Some(0.0),
                ..Default::default()
            },
            &mut state,
            &HashMap::new(),
            SR,
        );
        let committed = SR_F * 1.04;
        assert!((state.decks["A"].play_start_frame - committed).abs() < 1.0);
        assert!((state.decks["A"].nudge_factor - 1.0).abs() < 1e-9);
    }

    #[test]
    fn nudge_preserved_through_snapshot_round_trip() {
        let path = "/fake/track.mp3".to_string();
        let mut cache: SampleCache = HashMap::new();
        cache.insert(path.clone(), (Arc::new(vec![0.0f32; 441_000]), 1));

        let events = vec![
            SessionEvent {
                event_type: "deck_snapshot".to_string(),
                elapsed_ms: 0.0,
                deck: Some("A".to_string()),
                path: Some(path),
                is_playing: Some(true),
                playback_rate: Some(1.0),
                position_sec: Some(0.0),
                ..Default::default()
            },
            SessionEvent {
                event_type: "set_nudge".to_string(),
                elapsed_ms: 500.0,
                deck: Some("A".to_string()),
                percent: Some(4.0),
                ..Default::default()
            },
        ];
        let snaps = build_snapshots(&events, SR, &cache);
        // Last snapshot is after the nudge event: nudge_factor should be 1.04.
        let last = snaps.last().unwrap();
        let nf = last.decks.get("A").map(|d| d.nudge_factor).unwrap_or(0.0);
        assert!((nf - 1.04).abs() < 1e-9, "nudge_factor in snapshot: {nf}");

        // Round-trip: SimState from snapshot should carry nudge through to sim_pos.
        let sim = sim_state_from_snapshot(last);
        // From t=500ms at 1.04x for 500ms more → position = (500ms at 1.0x) + (500ms at 1.04x)
        // = 22050 + 22932 = 44982
        let expected = SR_F * 0.5 + SR_F * 0.5 * 1.04;
        let pos = sim_pos(&sim.decks["A"], 1000.0, SR_F);
        assert!(
            (pos - expected).abs() < 1.0,
            "pos={pos}, expected={expected}"
        );
    }

    // Session editing splices new events into the in-memory list and rebuilds
    // snapshots via update_session_events. These tests assert that snapshots
    // built from an edited list reflect the edit inside its range and the
    // original state after it, and that scrubbing stays consistent with
    // playthrough when rate edits are inserted mid-session.

    fn strip_gain_at(events: &[SessionEvent], from_ms: f64, cache: &SampleCache) -> f32 {
        let snaps = build_snapshots(events, SR, cache);
        let idx = snaps.partition_point(|snap| snap.elapsed_ms <= from_ms);
        let (mut sim, snapshot_ms) = match idx.checked_sub(1) {
            Some(snap_idx) => (
                sim_state_from_snapshot(&snaps[snap_idx]),
                snaps[snap_idx].elapsed_ms,
            ),
            None => (SimState::new(), 0.0),
        };
        let mut sorted: Vec<&SessionEvent> = events.iter().collect();
        sorted.sort_by(|left, right| left.elapsed_ms.partial_cmp(&right.elapsed_ms).unwrap());
        for ev in sorted
            .iter()
            .filter(|event| event.elapsed_ms > snapshot_ms && event.elapsed_ms <= from_ms)
        {
            sim_apply_event(ev, &mut sim, cache, SR);
        }
        sim.strips.get("A").map(|strip| strip.gain).unwrap_or(1.0)
    }

    #[test]
    fn edited_volume_applies_inside_range_and_restores_after() {
        let cache: SampleCache = HashMap::new();
        let vol = |at_ms: f64, gain: f32| SessionEvent {
            gain: Some(gain),
            ..deck_ev("set_volume", at_ms, "A")
        };

        let original = vec![vol(1000.0, 0.8)];
        // Splice result of drawing 0.4 over [5000, 8000]: inserted points plus
        // the restore event at t1 with the original value.
        let edited = vec![vol(1000.0, 0.8), vol(5000.0, 0.4), vol(8000.0, 0.8)];

        assert_eq!(strip_gain_at(&original, 3000.0, &cache), 0.8);
        assert_eq!(strip_gain_at(&edited, 3000.0, &cache), 0.8);
        assert_eq!(strip_gain_at(&edited, 6000.0, &cache), 0.4);
        assert_eq!(strip_gain_at(&edited, 9000.0, &cache), 0.8);
    }

    #[test]
    fn rate_edit_scrub_matches_playthrough() {
        let path = "/fake/a.wav".to_string();
        let mut cache: SampleCache = HashMap::new();
        cache.insert(
            path.clone(),
            (Arc::new(vec![0.0f32; 60 * SR as usize * 2]), 2),
        );

        let rate = |at_ms: f64, rate_value: f64| SessionEvent {
            rate: Some(rate_value),
            ..deck_ev("set_playback_rate", at_ms, "A")
        };

        // A drawn rate ramp over [5000, 9000] with a restore back to 1.0 at t1.
        let events = vec![
            SessionEvent {
                path: Some(path),
                is_playing: Some(true),
                playback_rate: Some(1.0),
                position_sec: Some(0.0),
                ..deck_ev("deck_snapshot", 0.0, "A")
            },
            rate(5000.0, 1.02),
            rate(6000.0, 1.05),
            rate(7000.0, 1.08),
            rate(8000.0, 1.04),
            rate(9000.0, 1.0),
        ];

        for step in 0..=150 {
            let at_ms = step as f64 * 100.0;
            let playthrough = playthrough_pos(&events, at_ms, &cache, "A");
            let scrub = scrub_pos(&events, at_ms, &cache, "A");
            match (playthrough, scrub) {
                (Some(playthrough_frame), Some(scrub_frame)) => assert!(
                    (playthrough_frame - scrub_frame).abs() < 1.0,
                    "t={at_ms}ms playthrough={playthrough_frame} scrub={scrub_frame} diff={}",
                    playthrough_frame - scrub_frame
                ),
                (None, None) => {}
                _ => panic!("t={at_ms}ms one path produced a deck and the other didn't"),
            }
        }
    }

    // ── event types previously without parity coverage ────────────────────────

    // loop_in must commit the current (possibly looped) position before
    // clearing loop state, and reloop must jump back to the loop start and
    // re-arm. A full cycle: loop_in → loop_out → wrap → exit_loop → reloop →
    // loop_in again while the loop is active.
    fn loop_cycle_events(path: &str) -> Vec<SessionEvent> {
        vec![
            playing_snapshot(path),
            SessionEvent {
                cue_sec: Some(4.0),
                ..deck_ev("loop_in", 4000.0, "A")
            },
            SessionEvent {
                start_sec: Some(4.0),
                end_sec: Some(6.0),
                ..deck_ev("loop_out", 6000.0, "A")
            },
            deck_ev("exit_loop", 11_000.0, "A"),
            deck_ev("reloop", 13_000.0, "A"),
            SessionEvent {
                cue_sec: Some(8.0),
                ..deck_ev("loop_in", 16_500.0, "A")
            },
        ]
    }

    #[test]
    fn scrub_matches_playthrough_for_loop_in_reloop_cycle() {
        let path = "/fake/a.wav";
        let cache = cached_track(path, 60);
        let events = loop_cycle_events(path);
        assert_scrub_matches_playthrough(&events, &cache, 200, "A");
    }

    #[test]
    fn load_track_starts_stopped_at_beat_offset() {
        let path = "/fake/a.wav";
        let cache = cached_track(path, 60);
        let events = vec![
            SessionEvent {
                path: Some(path.to_string()),
                beat_offset_sec: Some(2.0),
                ..deck_ev("load_track", 1000.0, "A")
            },
            deck_ev("play", 3000.0, "A"),
        ];

        // Stopped at the beat offset until play fires.
        let before_play = playthrough_pos(&events, 2000.0, &cache, "A").unwrap();
        assert!((before_play - 2.0 * SR_F).abs() < 1.0, "pos={before_play}");

        // play without sec resumes from the load position: 2s in + 2s played.
        let after_play = playthrough_pos(&events, 5000.0, &cache, "A").unwrap();
        assert!((after_play - 4.0 * SR_F).abs() < 1.0, "pos={after_play}");

        assert_scrub_matches_playthrough(&events, &cache, 100, "A");
    }

    // The live engine fully resets a deck on load_track (d.reset()), so a loop
    // region or nudge from the previous track can never survive a load. The
    // sim must match: a reloop after load_track is a no-op, not a jump back
    // into the previous track's loop.
    #[test]
    fn load_track_clears_loop_region_and_nudge() {
        let path = "/fake/a.wav";
        let cache = cached_track(path, 60);
        let events = vec![
            playing_snapshot(path),
            SessionEvent {
                start_sec: Some(4.0),
                end_sec: Some(6.0),
                ..deck_ev("loop_out", 6000.0, "A")
            },
            SessionEvent {
                percent: Some(4.0),
                ..deck_ev("set_nudge", 7000.0, "A")
            },
            SessionEvent {
                path: Some(path.to_string()),
                beat_offset_sec: Some(0.0),
                ..deck_ev("load_track", 8000.0, "A")
            },
            deck_ev("play", 9000.0, "A"),
            deck_ev("reloop", 10_000.0, "A"),
        ];

        // 3s of play at rate 1.0 with no nudge and no loop: position = 3s.
        let pos = playthrough_pos(&events, 12_000.0, &cache, "A").unwrap();
        assert!((pos - 3.0 * SR_F).abs() < 1.0, "pos={pos}");
    }

    #[test]
    fn eject_track_removes_deck_in_both_scrub_and_playthrough() {
        let path = "/fake/a.wav";
        let cache = cached_track(path, 60);
        let events = vec![playing_snapshot(path), deck_ev("eject_track", 5000.0, "A")];

        assert!(playthrough_pos(&events, 4000.0, &cache, "A").is_some());
        assert!(scrub_pos(&events, 4000.0, &cache, "A").is_some());
        assert!(playthrough_pos(&events, 6000.0, &cache, "A").is_none());
        assert!(scrub_pos(&events, 6000.0, &cache, "A").is_none());
    }

    #[test]
    fn stopped_at_cue_commits_cue_position() {
        let path = "/fake/a.wav";
        let cache = cached_track(path, 60);
        let events = vec![
            playing_snapshot(path),
            SessionEvent {
                cue_point_sec: Some(1.5),
                ..deck_ev("stopped_at_cue", 5000.0, "A")
            },
        ];

        let pos = playthrough_pos(&events, 8000.0, &cache, "A").unwrap();
        assert!((pos - 1.5 * SR_F).abs() < 1.0, "pos={pos}");
        assert_scrub_matches_playthrough(&events, &cache, 100, "A");
    }

    #[test]
    fn filter_change_captured_in_post_event_snapshot() {
        let events = vec![
            SessionEvent {
                value: Some(-0.5),
                ..deck_ev("set_filter", 1000.0, "A")
            },
            SessionEvent {
                active: Some(true),
                ..deck_ev("set_filter_active", 2000.0, "A")
            },
        ];
        let snaps = build_snapshots(&events, SR, &HashMap::new());
        let strip = snaps.last().unwrap().strips.get("A").unwrap();
        assert_eq!(strip.filter_value, -0.5);
        assert!(strip.filter_active);
    }

    #[test]
    fn filter_defaults_to_center_so_active_without_a_curve_is_transparent() {
        // Turning the filter on with no set_filter value must leave the knob at 0
        // (center/bypass), matching the timeline's DEFAULT_FILTER_VALUE. A stale
        // non-zero default would audibly filter where the drawn curve reads 0
        // (e.g. after stretching an active region back over an un-drawn stretch).
        let events = vec![SessionEvent {
            active: Some(true),
            ..deck_ev("set_filter_active", 1000.0, "A")
        }];
        let snaps = build_snapshots(&events, SR, &HashMap::new());
        let strip = snaps.last().unwrap().strips.get("A").unwrap();
        assert_eq!(strip.filter_value, 0.0);
        assert!(strip.filter_active);
    }

    #[test]
    fn beat_grid_change_captured_in_post_event_snapshot() {
        let path = "/fake/a.wav";
        let cache = cached_track(path, 60);
        let events = vec![
            playing_snapshot(path),
            SessionEvent {
                bpm: Some(128.0),
                beat_offset_sec: Some(0.25),
                ..deck_ev("set_beat_grid", 2000.0, "A")
            },
        ];
        let snaps = build_snapshots(&events, SR, &cache);
        let deck = snaps.last().unwrap().decks.get("A").unwrap();
        assert_eq!(deck.bpm, Some(128.0));
        assert!((deck.beat_offset_frames - 0.25 * SR_F).abs() < 1e-6);
    }
}
