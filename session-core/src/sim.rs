use crate::event::{SessionCommand, SessionEvent};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

// -2 dBFS: gives the master bus headroom before the hardware clipping point.
// 10^(-2/20) = 0.7943...
pub const DEFAULT_MASTER_GAIN: f32 = 0.7943;

// A percent below -100 would drive the step negative, and `next_pos` and `read_at` both
// assume forward motion. The engine floors to this too, or the two disagree.
pub const JOG_FACTOR_MIN: f64 = 0.1;

// The simulation only reads each track's frame count, never its samples.
pub type SampleCache = HashMap<String, (Arc<Vec<f32>>, usize)>;

#[derive(Clone)]
pub struct DeckSim {
    pub path: Option<String>,
    pub play_start_ms: f64,
    pub play_start_frame: f64,
    pub rate: f64,
    pub jog_hold_factor: f64,
    pub loop_active: bool,
    pub loop_end: f64,
    pub cue_point: f64,
    pub bpm: Option<f64>,
    pub beat_offset_frames: f64,
    pub total_frames: f64,
    pub is_playing: bool,
    // The travel a wheel gesture still owes, and when it was handed over. The engine
    // spreads it over the filter settle rather than applying it at once.
    pub jog_travel: f64,
    pub jog_ms: f64,
}

impl Default for DeckSim {
    fn default() -> Self {
        Self {
            path: None,
            play_start_ms: 0.0,
            play_start_frame: 0.0,
            rate: 1.0,
            jog_hold_factor: 1.0,
            loop_active: false,
            loop_end: 0.0,
            cue_point: 0.0,
            bpm: None,
            beat_offset_frames: 0.0,
            total_frames: 0.0,
            is_playing: false,
            jog_travel: 0.0,
            jog_ms: 0.0,
        }
    }
}

#[derive(Clone, Default)]
pub struct StripSim {
    /// Keyed `"slot/param"`, holding only what the session actually set. A strip starts at
    /// its manifest defaults, so an absent key means "left alone".
    pub params: BTreeMap<String, f32>,
    pub xfader_assign: crate::XfaderAssign,
}

impl StripSim {
    /// What the session set this address to, or `None` when it never touched it.
    pub fn param(&self, slot: &str, param: &str) -> Option<f32> {
        self.params.get(&format!("{slot}/{param}")).copied()
    }
}

pub struct SimState {
    pub decks: HashMap<String, DeckSim>,
    pub strips: HashMap<String, StripSim>,
    pub master_gain: f32,
    pub xfader_position: f32,
    pub fader_curve: crate::FaderCurve,
    pub jog_rotation_speed: crate::JogRotationSpeed,
}

impl Default for SimState {
    fn default() -> Self {
        Self {
            decks: HashMap::new(),
            strips: HashMap::new(),
            master_gain: DEFAULT_MASTER_GAIN,
            xfader_position: 0.0,
            fader_curve: crate::FaderCurve::default(),
            jog_rotation_speed: crate::JogRotationSpeed::default(),
        }
    }
}

impl SimState {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Clone, Default)]
pub struct DeckSnap {
    pub path: Option<String>,
    pub position_frame: f64,
    pub is_playing: bool,
    pub rate: f64,
    pub jog_hold_factor: f64,
    pub loop_active: bool,
    pub loop_end: f64,
    pub cue_point: f64,
    pub bpm: Option<f64>,
    pub beat_offset_frames: f64,
    pub total_frames: f64,
    // A scrub landing mid-gesture would otherwise lose whatever the wheel still owed.
    pub jog_travel: f64,
    pub jog_ms: f64,
}

#[derive(Clone)]
pub struct SessionSnapshot {
    pub elapsed_ms: f64,
    pub decks: HashMap<String, DeckSnap>,
    pub strips: HashMap<String, StripSim>,
    pub master_gain: f32,
    pub xfader_position: f32,
    pub fader_curve: crate::FaderCurve,
    pub jog_rotation_speed: crate::JogRotationSpeed,
}

pub fn current_beat(position_sec: f64, beat_offset_sec: f64, bpm: f64) -> f64 {
    if bpm <= 0.0 {
        return 0.0;
    }
    (position_sec - beat_offset_sec) * bpm / 60.0
}

/// How much of a gesture has arrived by `ms`. The engine's one-pole reaches this same
/// total, so the two only differ inside the settle by less than one block.
fn jog_settled(sim: &DeckSim, ms: f64) -> f64 {
    if sim.jog_travel == 0.0 {
        return 0.0;
    }
    let elapsed = (ms - sim.jog_ms).max(0.0) / 1000.0;
    sim.jog_travel * crate::jog_settled_fraction(elapsed)
}

/// Banks the position reached so far and leaves the gesture its unsettled remainder, so
/// an event landing mid-scrub neither drops the rest nor replays what already arrived.
fn commit_pos(sim: &mut DeckSim, ms: f64, sample_rate_f64: f64) {
    let committed = sim_pos(sim, ms, sample_rate_f64);
    let remaining = sim.jog_travel - jog_settled(sim, ms);
    sim.play_start_frame = committed;
    sim.play_start_ms = ms;
    sim.jog_travel = remaining;
    sim.jog_ms = ms;
}

pub fn sim_pos(sim: &DeckSim, ms: f64, sample_rate_f64: f64) -> f64 {
    if !sim.is_playing {
        let settled = jog_settled(sim, ms);
        if settled == 0.0 {
            return sim.play_start_frame;
        }
        return (sim.play_start_frame + settled).clamp(0.0, sim.total_frames);
    }
    let effective_rate = sim.rate * sim.jog_hold_factor;
    let elapsed = (ms - sim.play_start_ms).max(0.0) / 1000.0 * sample_rate_f64 * effective_rate;
    let raw = sim.play_start_frame + elapsed + jog_settled(sim, ms);
    // Engine parity: play linearly until loop_end, only then wrap (deck.rs next_pos).
    if sim.loop_active && sim.loop_end > sim.cue_point && raw >= sim.loop_end {
        let len = sim.loop_end - sim.cue_point;
        let wrapped = sim.cue_point + (raw - sim.loop_end).rem_euclid(len);
        wrapped.clamp(0.0, sim.total_frames)
    } else {
        raw.clamp(0.0, sim.total_frames)
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
                    jog_hold_factor: d.jog_hold_factor,
                    loop_active: d.loop_active,
                    loop_end: d.loop_end,
                    cue_point: d.cue_point,
                    bpm: d.bpm,
                    beat_offset_frames: d.beat_offset_frames,
                    total_frames: d.total_frames,
                    is_playing: d.is_playing,
                    jog_travel: d.jog_travel,
                    jog_ms: d.jog_ms,
                },
            )
        })
        .collect();

    let strips = snap.strips.clone();

    SimState {
        decks,
        strips,
        master_gain: snap.master_gain,
        jog_rotation_speed: snap.jog_rotation_speed,
        xfader_position: snap.xfader_position,
        fader_curve: snap.fader_curve,
    }
}

pub fn sim_apply_event(
    event: &SessionEvent,
    state: &mut SimState,
    cache: &SampleCache,
    sample_rate: u32,
) {
    let sample_rate_f64 = sample_rate as f64;
    let Some(cmd) = event.command() else { return };

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
                .map(|(samples, channels)| samples.len() as f64 / *channels as f64)
                .unwrap_or(0.0);
            let sim = state.decks.entry(deck.to_string()).or_default();
            sim.path = Some(path.to_string());
            sim.total_frames = total_frames;
            sim.rate = playback_rate.unwrap_or(1.0);
            let pos = position_sec.unwrap_or(0.0) * sample_rate_f64;
            sim.play_start_frame = pos;
            sim.play_start_ms = 0.0;
            sim.is_playing = is_playing;
            sim.loop_active = loop_active.unwrap_or(false);
            sim.cue_point = cue_point_sec.map_or(0.0, |sec| sec * sample_rate_f64);
            sim.loop_end = loop_end_sec.map_or(0.0, |sec| sec * sample_rate_f64);
            sim.bpm = bpm;
        }
        SessionCommand::LoadTrack {
            deck,
            path,
            beat_offset_sec,
        } => {
            let total_frames = cache
                .get(path)
                .map(|(samples, channels)| samples.len() as f64 / *channels as f64)
                .unwrap_or(0.0);
            let sim = state.decks.entry(deck.to_string()).or_default();
            sim.path = Some(path.to_string());
            sim.total_frames = total_frames;
            sim.rate = 1.0;
            let pos = beat_offset_sec.unwrap_or(0.0) * sample_rate_f64;
            sim.play_start_frame = pos;
            sim.play_start_ms = event.elapsed_ms;
            sim.is_playing = false;
            sim.loop_active = false;
            // The live engine fully resets the deck on load: no loop region
            // or nudge survives into the new track.
            sim.loop_end = 0.0;
            sim.jog_hold_factor = 1.0;
            sim.cue_point = pos;
            sim.bpm = None;
            sim.beat_offset_frames = pos;
        }
        SessionCommand::Play { deck, sec } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            commit_pos(sim, event.elapsed_ms, sample_rate_f64);
            if let Some(sec) = sec {
                sim.play_start_frame = sec * sample_rate_f64;
            }
            sim.is_playing = true;
        }
        SessionCommand::Stop { deck } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            commit_pos(sim, event.elapsed_ms, sample_rate_f64);
            sim.is_playing = false;
        }
        SessionCommand::StopAtCue {
            deck,
            cue_point_sec,
        } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            commit_pos(sim, event.elapsed_ms, sample_rate_f64);
            if let Some(sec) = cue_point_sec {
                sim.play_start_frame = sec * sample_rate_f64;
            }
            sim.is_playing = false;
        }
        SessionCommand::Seek { deck, sec } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            commit_pos(sim, event.elapsed_ms, sample_rate_f64);
            sim.play_start_frame = sec * sample_rate_f64;
            sim.loop_active = sim.loop_active
                && (sec * sample_rate_f64 >= sim.cue_point)
                && (sec * sample_rate_f64 < sim.loop_end);
        }
        SessionCommand::SetPlaybackRate { deck, rate } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            commit_pos(sim, event.elapsed_ms, sample_rate_f64);
            sim.rate = rate.max(0.1);
        }
        SessionCommand::SetXfaderAssign { deck, assign } => {
            state
                .strips
                .entry(deck.to_string())
                .or_default()
                .xfader_assign = assign;
        }
        SessionCommand::SetFaderCurve { curve } => {
            state.fader_curve = curve;
        }
        SessionCommand::SetJogRotationSpeed { speed } => {
            state.jog_rotation_speed = speed;
        }
        // The engine spreads this travel over the wheel filter's settle, which is silent on
        // a paused deck and a brief bend on a playing one. Only the total reaches the sim.
        SessionCommand::Jog { deck, ticks } => {
            let speed = state.jog_rotation_speed;
            let sim = state.decks.entry(deck.to_string()).or_default();
            let travel = ticks * speed.frames_per_tick(sample_rate_f64);
            let moved = if sim.is_playing {
                sim.rate * travel / crate::JOG_PAUSED_MULTIPLIER
            } else {
                travel
            };
            commit_pos(sim, event.elapsed_ms, sample_rate_f64);
            sim.jog_travel += moved;
        }
        SessionCommand::SetNudge { deck, percent } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            commit_pos(sim, event.elapsed_ms, sample_rate_f64);
            sim.jog_hold_factor = (1.0 + percent / 100.0).max(JOG_FACTOR_MIN);
        }
        SessionCommand::LoopIn { deck, cue_sec } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            // Commit the current (possibly looped) position before clearing
            // loop_active, otherwise sim_pos would fall back to its raw linear
            // anchor and jump to where the deck would be had it never looped.
            commit_pos(sim, event.elapsed_ms, sample_rate_f64);
            if let Some(cue_sec) = cue_sec {
                sim.cue_point = cue_sec * sample_rate_f64;
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
            if let Some(start_sec) = start_sec {
                sim.cue_point = start_sec * sample_rate_f64;
            }
            if let Some(end_sec) = end_sec {
                sim.loop_end = end_sec * sample_rate_f64;
            }
            sim.loop_active = true;
        }
        SessionCommand::ExitLoop { deck } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            // Commit the current looped position before leaving the loop, so the
            // subsequent linear sim_pos continues from inside the loop region
            // instead of jumping to the un-looped linear position.
            commit_pos(sim, event.elapsed_ms, sample_rate_f64);
            sim.loop_active = false;
        }
        SessionCommand::Reloop { deck } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            if sim.loop_end > sim.cue_point {
                sim.play_start_frame = sim.cue_point;
                sim.play_start_ms = event.elapsed_ms;
                if sim.is_playing {
                    sim.loop_active = true;
                }
            }
        }
        SessionCommand::EjectTrack { deck } => {
            state.decks.remove(deck);
        }
        SessionCommand::SetParam {
            deck,
            slot,
            param,
            value,
            ..
        } => match (deck, slot, param) {
            (Some(deck), _, _) => {
                state
                    .strips
                    .entry(deck.to_string())
                    .or_default()
                    .params
                    .insert(format!("{slot}/{param}"), value as f32);
            }
            (None, "gain", "gain") => state.master_gain = value as f32,
            (None, "xfader", "position") => state.xfader_position = value as f32,
            (None, _, _) => {}
        },
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
                sim.beat_offset_frames = off * sample_rate_f64;
            }
        }
        SessionCommand::CuePreviewStart {
            deck,
            cue_point_sec,
        } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            let cue_frame = cue_point_sec
                .map(|sec| sec * sample_rate_f64)
                .unwrap_or(sim.cue_point);
            sim.cue_point = cue_frame;
            sim.play_start_frame = cue_frame;
            sim.play_start_ms = event.elapsed_ms;
            sim.is_playing = true;
        }
        SessionCommand::CuePreviewEnd {
            deck,
            cue_point_sec,
        } => {
            let sim = state.decks.entry(deck.to_string()).or_default();
            let cue_frame = cue_point_sec
                .map(|sec| sec * sample_rate_f64)
                .unwrap_or(sim.cue_point);
            sim.play_start_frame = cue_frame;
            sim.play_start_ms = event.elapsed_ms;
            sim.is_playing = false;
        }
    }
}

fn snap_at(state: &SimState, ms: f64, sample_rate_f64: f64) -> SessionSnapshot {
    let decks = state
        .decks
        .iter()
        .map(|(id, sim)| {
            (
                id.clone(),
                DeckSnap {
                    path: sim.path.clone(),
                    position_frame: sim_pos(sim, ms, sample_rate_f64),
                    is_playing: sim.is_playing,
                    rate: sim.rate,
                    jog_hold_factor: sim.jog_hold_factor,
                    loop_active: sim.loop_active,
                    loop_end: sim.loop_end,
                    cue_point: sim.cue_point,
                    bpm: sim.bpm,
                    beat_offset_frames: sim.beat_offset_frames,
                    total_frames: sim.total_frames,
                    jog_travel: sim.jog_travel - jog_settled(sim, ms),
                    jog_ms: ms,
                },
            )
        })
        .collect();

    let strips = state.strips.clone();

    SessionSnapshot {
        elapsed_ms: ms,
        decks,
        strips,
        master_gain: state.master_gain,
        xfader_position: state.xfader_position,
        fader_curve: state.fader_curve,
        jog_rotation_speed: state.jog_rotation_speed,
    }
}

// deck_snapshot (initial state) sorts first within its rounded-ms cluster, and
// at an exactly equal timestamp (only edits synthesize those) transport enders
// sort before starters. Both rules are pinned by tests in this module.
pub fn sorted_by_sim_order(mut events: Vec<SessionEvent>) -> Vec<SessionEvent> {
    events.sort_by(event_sim_order);
    events
}

pub fn event_sim_order(a: &SessionEvent, b: &SessionEvent) -> std::cmp::Ordering {
    let bucket = |event: &SessionEvent| event.elapsed_ms.round() as i64;
    let snapshot_rank = |event: &SessionEvent| u8::from(event.event_type != "deck_snapshot");
    bucket(a)
        .cmp(&bucket(b))
        .then_with(|| snapshot_rank(a).cmp(&snapshot_rank(b)))
        .then_with(|| {
            a.elapsed_ms
                .partial_cmp(&b.elapsed_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then_with(|| transport_phase(a).cmp(&transport_phase(b)))
}

/// Enders before starters, so a stop and a play sharing an instant leave the deck
/// stopped rather than started.
pub fn transport_phase(event: &SessionEvent) -> u8 {
    match event.event_type.as_str() {
        "stop" | "stopped_at_cue" | "exit_loop" | "cue_preview_end" => 0,
        "play" | "loop_out" | "cue_preview_start" | "reloop" => 2,
        _ => 1,
    }
}

// One snapshot per event, captured after it fires, so a scrub lands on exact state rather
// than replaying from the last checkpoint.
pub fn build_snapshots(
    events: &[SessionEvent],
    sample_rate: u32,
    cache: &SampleCache,
) -> Vec<SessionSnapshot> {
    let sample_rate_f64 = sample_rate as f64;
    let mut state = SimState::new();
    let mut snapshots = Vec::new();

    let mut sorted: Vec<&SessionEvent> = events.iter().collect();
    sorted.sort_by(|a, b| event_sim_order(a, b));

    // Snapshot at t=0 before any events (clean initial state).
    snapshots.push(snap_at(&state, 0.0, sample_rate_f64));

    for event in sorted {
        sim_apply_event(event, &mut state, cache, sample_rate);
        // Snapshot AFTER the event: scrubbing to event.elapsed_ms loads the
        // fully-applied post-event state, and the timer loop only replays
        // events strictly after from_ms.
        snapshots.push(snap_at(&state, event.elapsed_ms, sample_rate_f64));
    }

    snapshots
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: u32 = 44100;
    const SAMPLE_RATE_F64: f64 = 44100.0;

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
            (
                Arc::new(vec![0.0f32; seconds * SAMPLE_RATE as usize * 2]),
                2,
            ),
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

    // Scrubbing (snapshot + replay, as start_session_playback does) must land
    // every deck exactly where playing through all events would.
    fn playthrough_pos(
        events: &[SessionEvent],
        from_ms: f64,
        cache: &SampleCache,
        deck: &str,
    ) -> Option<f64> {
        let mut sorted: Vec<&SessionEvent> = events.iter().collect();
        sorted.sort_by(|a, b| a.elapsed_ms.partial_cmp(&b.elapsed_ms).unwrap());
        let mut state = SimState::new();
        for event in sorted.iter().filter(|event| event.elapsed_ms <= from_ms) {
            sim_apply_event(event, &mut state, cache, SAMPLE_RATE);
        }
        state
            .decks
            .get(deck)
            .map(|d| sim_pos(d, from_ms, SAMPLE_RATE_F64))
    }

    fn scrub_pos(
        events: &[SessionEvent],
        from_ms: f64,
        cache: &SampleCache,
        deck: &str,
    ) -> Option<f64> {
        let snaps = build_snapshots(events, SAMPLE_RATE, cache);
        let idx = snaps.partition_point(|s| s.elapsed_ms <= from_ms);
        let (mut sim, snapshot_ms) = match idx.checked_sub(1) {
            Some(i) => (sim_state_from_snapshot(&snaps[i]), snaps[i].elapsed_ms),
            None => (SimState::new(), 0.0),
        };
        let mut sorted: Vec<&SessionEvent> = events.iter().collect();
        sorted.sort_by(|a, b| a.elapsed_ms.partial_cmp(&b.elapsed_ms).unwrap());
        for event in sorted
            .iter()
            .filter(|event| event.elapsed_ms > snapshot_ms && event.elapsed_ms <= from_ms)
        {
            sim_apply_event(event, &mut sim, cache, SAMPLE_RATE);
        }
        sim.decks
            .get(deck)
            .map(|d| sim_pos(d, from_ms, SAMPLE_RATE_F64))
    }

    fn assert_scrub_matches_playthrough(
        events: &[SessionEvent],
        cache: &SampleCache,
        steps: usize,
        deck: &str,
    ) {
        for step in 0..=steps {
            let ms = step as f64 * 100.0;
            let playthrough = playthrough_pos(events, ms, cache, deck);
            let scrub = scrub_pos(events, ms, cache, deck);
            match (playthrough, scrub) {
                (Some(playthrough_frame), Some(scrub_frame)) => assert!(
                    (playthrough_frame - scrub_frame).abs() < 1.0,
                    "t={ms}ms playthrough={playthrough_frame} scrub={scrub_frame} diff={}",
                    playthrough_frame - scrub_frame
                ),
                (None, None) => {}
                _ => panic!("t={ms}ms one path produced a deck and the other didn't"),
            }
        }
    }

    #[test]
    fn scrub_matches_playthrough_for_realistic_session() {
        let path = "/fake/a.wav".to_string();
        let mut cache: SampleCache = HashMap::new();
        cache.insert(
            path.clone(),
            (Arc::new(vec![0.0f32; 60 * SAMPLE_RATE as usize * 2]), 2),
        );

        let make_event = |t: f64, event_type: &str, deck: &str| SessionEvent {
            event_type: event_type.to_string(),
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
                ..make_event(0.0, "deck_snapshot", "A")
            },
            SessionEvent {
                rate: Some(1.03),
                ..make_event(4000.0, "set_playback_rate", "A")
            },
            SessionEvent {
                start_sec: Some(8.0),
                end_sec: Some(10.0),
                ..make_event(10_000.0, "loop_out", "A")
            },
            make_event(18_000.0, "exit_loop", "A"),
            SessionEvent {
                sec: Some(20.0),
                ..make_event(20_000.0, "seek", "A")
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
        let snaps = build_snapshots(events, SAMPLE_RATE, cache);
        let idx = snaps.partition_point(|s| s.elapsed_ms <= from_ms);
        let (mut sim, snapshot_ms) = match idx.checked_sub(1) {
            Some(i) => (sim_state_from_snapshot(&snaps[i]), snaps[i].elapsed_ms),
            None => (SimState::new(), 0.0),
        };
        let mut sorted: Vec<&SessionEvent> = events.iter().collect();
        sorted.sort_by(|a, b| a.elapsed_ms.partial_cmp(&b.elapsed_ms).unwrap());
        for event in sorted.iter().filter(|event| {
            event.elapsed_ms > snapshot_ms
                && event.elapsed_ms <= from_ms
                && event.event_type != "deck_snapshot"
        }) {
            sim_apply_event(event, &mut sim, cache, SAMPLE_RATE);
        }
        sim.decks
            .get(deck)
            .map(|d| (d.is_playing, d.path.clone()))
            .unwrap_or((false, None))
    }

    #[test]
    fn collapsed_load_play_at_start_reconstructs_playing() {
        let track1 = "/fake/track1.wav".to_string();
        let track2 = "/fake/track2.wav".to_string();
        let mut cache: SampleCache = HashMap::new();
        cache.insert(
            track1.clone(),
            (Arc::new(vec![0.0f32; 60 * SAMPLE_RATE as usize * 2]), 2),
        );
        cache.insert(
            track2.clone(),
            (Arc::new(vec![0.0f32; 60 * SAMPLE_RATE as usize * 2]), 2),
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

    #[test]
    fn sim_pos_stopped_returns_start_frame() {
        let sim = DeckSim {
            is_playing: false,
            play_start_frame: 5000.0,
            ..Default::default()
        };
        assert_eq!(sim_pos(&sim, 99999.0, SAMPLE_RATE_F64), 5000.0);
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
        assert_eq!(sim_pos(&sim, 1000.0, SAMPLE_RATE_F64), SAMPLE_RATE_F64);
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
        assert_eq!(
            sim_pos(&sim, 1000.0, SAMPLE_RATE_F64),
            SAMPLE_RATE_F64 * 2.0
        );
    }

    #[test]
    fn sim_pos_advances_from_nonzero_start_time() {
        let sim = DeckSim {
            is_playing: true,
            play_start_ms: 500.0,
            play_start_frame: SAMPLE_RATE_F64 / 2.0,
            rate: 1.0,
            total_frames: 1_000_000.0,
            ..Default::default()
        };
        assert_eq!(sim_pos(&sim, 1000.0, SAMPLE_RATE_F64), SAMPLE_RATE_F64);
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
        assert_eq!(sim_pos(&sim, 10000.0, SAMPLE_RATE_F64), 100.0);
    }

    #[test]
    fn sim_pos_loop_wraps_at_boundary() {
        let sim = DeckSim {
            is_playing: true,
            play_start_ms: 0.0,
            play_start_frame: 0.0,
            rate: 1.0,
            loop_active: true,
            loop_end: SAMPLE_RATE_F64,
            total_frames: 1_000_000.0,
            ..Default::default()
        };
        let pos = sim_pos(&sim, 1500.0, SAMPLE_RATE_F64);
        assert!(
            (pos - SAMPLE_RATE_F64 / 2.0).abs() < 1.0,
            "expected ~22050, got {pos}"
        );
    }

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
            SAMPLE_RATE,
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
            SAMPLE_RATE,
        );
        assert!(!state.decks["A"].is_playing);
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
            SAMPLE_RATE,
        );
        assert_eq!(state.decks["A"].play_start_frame, 10.0 * SAMPLE_RATE_F64);
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
            SAMPLE_RATE,
        );
        assert_eq!(state.decks["A"].play_start_frame, SAMPLE_RATE_F64);
        assert_eq!(state.decks["A"].rate, 2.0);
        assert_eq!(
            sim_pos(&state.decks["A"], 2000.0, SAMPLE_RATE_F64),
            132300.0
        );
    }

    #[test]
    fn apply_fader_gain_updates_strip_gain() {
        let mut state = SimState::new();
        sim_apply_event(
            &SessionEvent::param(0.0, Some("A"), "fader", "gain", 0.5),
            &mut state,
            &HashMap::new(),
            SAMPLE_RATE,
        );
        assert_eq!(state.strips["A"].param("fader", "gain").unwrap_or(1.0), 0.5);
    }

    #[test]
    fn apply_eq_params_update_all_three_bands() {
        let mut state = SimState::new();
        for (band, db) in [("low", -3.0f32), ("mid", -6.0), ("high", -9.0)] {
            sim_apply_event(
                &SessionEvent::param(0.0, Some("A"), "eq", band, db as f64),
                &mut state,
                &HashMap::new(),
                SAMPLE_RATE,
            );
        }
        assert_eq!(state.strips["A"].param("eq", "low").unwrap_or(0.0), -3.0);
        assert_eq!(state.strips["A"].param("eq", "mid").unwrap_or(0.0), -6.0);
        assert_eq!(state.strips["A"].param("eq", "high").unwrap_or(0.0), -9.0);
    }

    #[test]
    fn apply_master_gain_param() {
        let mut state = SimState::new();
        sim_apply_event(
            &SessionEvent::param(0.0, None, "gain", "gain", 0.5),
            &mut state,
            &HashMap::new(),
            SAMPLE_RATE,
        );
        assert_eq!(state.master_gain, 0.5);
    }

    #[test]
    fn empty_session_produces_single_clean_snapshot() {
        let snaps = build_snapshots(&[], SAMPLE_RATE, &HashMap::new());
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
        let snaps = build_snapshots(&events, SAMPLE_RATE, &HashMap::new());
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
        let snaps = build_snapshots(&events, SAMPLE_RATE, &HashMap::new());
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
        let snaps = build_snapshots(&events, SAMPLE_RATE, &HashMap::new());
        for i in 1..snaps.len() {
            assert!(
                snaps[i].elapsed_ms >= snaps[i - 1].elapsed_ms,
                "snapshot[{i}] ({}) < snapshot[{}] ({})",
                snaps[i].elapsed_ms,
                i - 1,
                snaps[i - 1].elapsed_ms,
            );
        }
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
        let events = vec![SessionEvent::param(3000.0, Some("A"), "eq", "low", -6.0)];
        let snaps = build_snapshots(&events, SAMPLE_RATE, &HashMap::new());
        assert_eq!(
            snaps[0]
                .strips
                .get("A")
                .and_then(|s| s.param("eq", "low"))
                .unwrap_or(0.0),
            0.0
        );
        assert_eq!(
            snaps
                .last()
                .unwrap()
                .strips
                .get("A")
                .and_then(|s| s.param("eq", "low"))
                .unwrap_or(0.0),
            -6.0
        );
    }

    #[test]
    fn scrub_before_first_event_loads_clean_state() {
        let events = vec![SessionEvent {
            event_type: "play".to_string(),
            elapsed_ms: 5000.0,
            deck: Some("A".to_string()),
            ..Default::default()
        }];
        let snaps = build_snapshots(&events, SAMPLE_RATE, &HashMap::new());
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
            SessionEvent::param(5000.0, Some("A"), "eq", "low", -6.0),
        ];
        let snaps = build_snapshots(&events, SAMPLE_RATE, &HashMap::new());
        // At 3000ms: after play, before EQ change.
        let idx = snaps.partition_point(|s| s.elapsed_ms <= 3000.0);
        let snap = &snaps[idx - 1];
        assert_eq!(snap.elapsed_ms, 1000.0);
        assert!(snap.decks.get("A").map(|d| d.is_playing).unwrap_or(false));
        assert_eq!(
            snap.strips
                .get("A")
                .and_then(|s| s.param("eq", "low"))
                .unwrap_or(0.0),
            0.0
        );
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
        let snaps = build_snapshots(&events, SAMPLE_RATE, &HashMap::new());
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
        let snaps = build_snapshots(&events, SAMPLE_RATE, &cache);
        let snap = snaps.last().unwrap();
        let sim = sim_state_from_snapshot(snap);
        assert_eq!(sim_pos(&sim.decks["A"], 2000.0, SAMPLE_RATE_F64), 88200.0);
    }

    #[test]
    fn sim_pos_accounts_for_jog_hold_factor() {
        // Nudge at +4% means the deck advances 4% faster.
        let sim = DeckSim {
            is_playing: true,
            play_start_ms: 0.0,
            play_start_frame: 0.0,
            rate: 1.0,
            jog_hold_factor: 1.04,
            total_frames: 1_000_000.0,
            ..Default::default()
        };
        let pos = sim_pos(&sim, 1000.0, SAMPLE_RATE_F64);
        assert_eq!(pos, SAMPLE_RATE_F64 * 1.04);
    }

    #[test]
    fn sim_pos_unit_nudge_unchanged() {
        let sim = DeckSim {
            is_playing: true,
            play_start_ms: 0.0,
            play_start_frame: 0.0,
            rate: 1.0,
            jog_hold_factor: 1.0,
            total_frames: 1_000_000.0,
            ..Default::default()
        };
        assert_eq!(sim_pos(&sim, 1000.0, SAMPLE_RATE_F64), SAMPLE_RATE_F64);
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
                jog_hold_factor: 1.0,
                total_frames: 1_000_000.0,
                ..Default::default()
            },
        );
        // Apply +4% nudge at t=1000ms. Position at that moment = 44100.
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
            SAMPLE_RATE,
        );
        assert_eq!(state.decks["A"].play_start_frame, SAMPLE_RATE_F64);
        assert_eq!(state.decks["A"].play_start_ms, 1000.0);
        assert!((state.decks["A"].jog_hold_factor - 1.04).abs() < 1e-9);

        // At t=2000ms: 44100 + 44100 * 1.04 = 44100 + 45864 = 89964.
        let pos = sim_pos(&state.decks["A"], 2000.0, SAMPLE_RATE_F64);
        assert_eq!(pos, SAMPLE_RATE_F64 + SAMPLE_RATE_F64 * 1.04);
    }

    fn jogged(is_playing: bool, rate: f64, ticks: f64, speed: &str) -> SimState {
        let mut state = SimState::new();
        state.decks.insert(
            "A".to_string(),
            DeckSim {
                is_playing,
                play_start_ms: 0.0,
                play_start_frame: 0.0,
                rate,
                total_frames: 100_000_000.0,
                ..Default::default()
            },
        );
        for event in [
            SessionEvent {
                event_type: "set_jog_rotation_speed".to_string(),
                speed: Some(speed.to_string()),
                ..Default::default()
            },
            SessionEvent {
                event_type: "jog".to_string(),
                elapsed_ms: 1000.0,
                deck: Some("A".to_string()),
                ticks: Some(ticks),
                ..Default::default()
            },
        ] {
            sim_apply_event(&event, &mut state, &HashMap::new(), SAMPLE_RATE);
        }
        state
    }

    #[test]
    fn a_paused_scrub_moves_the_playhead_by_its_tick_count() {
        let state = jogged(false, 1.0, 6.0, "rpm33");
        let expected = 6.0 * crate::JOG_SCRUB_SEC_PER_TICK_AT_33 * SAMPLE_RATE_F64;
        let settled = sim_pos(&state.decks["A"], 3000.0, SAMPLE_RATE_F64);
        assert!(
            (settled - expected).abs() < 1e-6,
            "settled {settled}, want {expected}"
        );
    }

    // A scrub that has already settled has moved the playhead, so an event that then
    // sets a position must bank it and not leave it pending to be added a second time.
    fn scrubbed_then(follow_up: SessionEvent) -> DeckSim {
        let mut state = jogged(false, 1.0, 1000.0, "rpm33");
        sim_apply_event(&follow_up, &mut state, &HashMap::new(), SAMPLE_RATE);
        state.decks.remove("A").unwrap()
    }

    #[test]
    fn play_after_a_scrub_starts_from_the_scrubbed_position() {
        let deck = scrubbed_then(SessionEvent {
            event_type: "play".to_string(),
            elapsed_ms: 2000.0,
            deck: Some("A".to_string()),
            ..Default::default()
        });
        let scrubbed = 1000.0 * crate::JOG_SCRUB_SEC_PER_TICK_AT_33 * SAMPLE_RATE_F64;
        assert!((sim_pos(&deck, 2000.0, SAMPLE_RATE_F64) - scrubbed).abs() < 1e-3);
    }

    #[test]
    fn seek_after_a_scrub_lands_on_the_seek_target() {
        let deck = scrubbed_then(SessionEvent {
            event_type: "seek".to_string(),
            elapsed_ms: 2000.0,
            deck: Some("A".to_string()),
            sec: Some(30.0),
            ..Default::default()
        });
        assert!((sim_pos(&deck, 2000.0, SAMPLE_RATE_F64) - 30.0 * SAMPLE_RATE_F64).abs() < 1e-3);
    }

    #[test]
    fn stop_at_cue_after_a_scrub_lands_on_the_cue_point() {
        let deck = scrubbed_then(SessionEvent {
            event_type: "stopped_at_cue".to_string(),
            elapsed_ms: 2000.0,
            deck: Some("A".to_string()),
            cue_point_sec: Some(12.0),
            ..Default::default()
        });
        assert!((sim_pos(&deck, 2000.0, SAMPLE_RATE_F64) - 12.0 * SAMPLE_RATE_F64).abs() < 1e-3);
    }

    #[test]
    fn the_rotation_speed_decides_what_a_tick_is_worth() {
        let settled = |speed| {
            sim_pos(
                &jogged(false, 1.0, 6.0, speed).decks["A"],
                3000.0,
                SAMPLE_RATE_F64,
            )
        };
        let at_33 = settled("rpm33");
        let at_45 = settled("rpm45");
        assert!(at_45 < at_33 && at_45 > 0.0, "33 {at_33}, 45 {at_45}");
    }

    #[test]
    fn a_playing_bend_moves_the_playhead_by_the_scrub_over_the_multiplier() {
        let scrub = 6.0 * crate::JOG_SCRUB_SEC_PER_TICK_AT_33 * SAMPLE_RATE_F64;
        let expected = 1.08 * scrub / crate::JOG_PAUSED_MULTIPLIER;
        // Read well past the settle, against the same deck with an untouched wheel.
        let at = |ticks| {
            sim_pos(
                &jogged(true, 1.08, ticks, "rpm33").decks["A"],
                4000.0,
                SAMPLE_RATE_F64,
            )
        };
        let moved = at(6.0) - at(0.0);
        assert!(
            (moved - expected).abs() < 1e-6,
            "moved {moved}, want {expected}"
        );
    }

    #[test]
    fn a_nudge_below_the_floor_is_floored_the_way_the_engine_floors_it() {
        let mut state = SimState::new();
        state.decks.insert(
            "A".to_string(),
            DeckSim {
                is_playing: true,
                play_start_ms: 0.0,
                play_start_frame: 0.0,
                rate: 1.0,
                jog_hold_factor: 1.0,
                total_frames: 1_000_000.0,
                ..Default::default()
            },
        );
        sim_apply_event(
            &SessionEvent {
                event_type: "set_nudge".to_string(),
                elapsed_ms: 0.0,
                deck: Some("A".to_string()),
                percent: Some(-200.0),
                ..Default::default()
            },
            &mut state,
            &HashMap::new(),
            SAMPLE_RATE,
        );

        assert_eq!(state.decks["A"].jog_hold_factor, JOG_FACTOR_MIN);
        let pos = sim_pos(&state.decks["A"], 1000.0, SAMPLE_RATE_F64);
        assert!(pos > 0.0, "a floored nudge still advances, got {pos}");
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
                jog_hold_factor: 1.04, // nudge already active
                total_frames: 1_000_000.0,
                ..Default::default()
            },
        );
        // Release nudge at t=1000ms. Position = 1000ms * 1.04 * sample_rate.
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
            SAMPLE_RATE,
        );
        let committed = SAMPLE_RATE_F64 * 1.04;
        assert!((state.decks["A"].play_start_frame - committed).abs() < 1.0);
        assert!((state.decks["A"].jog_hold_factor - 1.0).abs() < 1e-9);
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
        let snaps = build_snapshots(&events, SAMPLE_RATE, &cache);
        // Last snapshot is after the nudge event: jog_hold_factor should be 1.04.
        let last = snaps.last().unwrap();
        let jog_hold_factor = last
            .decks
            .get("A")
            .map(|d| d.jog_hold_factor)
            .unwrap_or(0.0);
        assert!(
            (jog_hold_factor - 1.04).abs() < 1e-9,
            "jog_hold_factor in snapshot: {jog_hold_factor}"
        );

        // Round-trip: SimState from snapshot should carry nudge through to sim_pos.
        let sim = sim_state_from_snapshot(last);
        // From t=500ms at 1.04x for 500ms more → position = (500ms at 1.0x) + (500ms at 1.04x)
        // = 22050 + 22932 = 44982
        let expected = SAMPLE_RATE_F64 * 0.5 + SAMPLE_RATE_F64 * 0.5 * 1.04;
        let pos = sim_pos(&sim.decks["A"], 1000.0, SAMPLE_RATE_F64);
        assert!(
            (pos - expected).abs() < 1.0,
            "pos={pos}, expected={expected}"
        );
    }

    fn strip_gain_at(events: &[SessionEvent], from_ms: f64, cache: &SampleCache) -> f32 {
        let snaps = build_snapshots(events, SAMPLE_RATE, cache);
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
        for event in sorted
            .iter()
            .filter(|event| event.elapsed_ms > snapshot_ms && event.elapsed_ms <= from_ms)
        {
            sim_apply_event(event, &mut sim, cache, SAMPLE_RATE);
        }
        sim.strips
            .get("A")
            .and_then(|strip| strip.param("fader", "gain"))
            .unwrap_or(1.0)
    }

    #[test]
    fn edited_volume_applies_inside_range_and_restores_after() {
        let cache: SampleCache = HashMap::new();
        let vol = |ms: f64, gain: f64| SessionEvent::param(ms, Some("A"), "fader", "gain", gain);

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
            (Arc::new(vec![0.0f32; 60 * SAMPLE_RATE as usize * 2]), 2),
        );

        let rate = |ms: f64, rate_value: f64| SessionEvent {
            rate: Some(rate_value),
            ..deck_ev("set_playback_rate", ms, "A")
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
            let ms = step as f64 * 100.0;
            let playthrough = playthrough_pos(&events, ms, &cache, "A");
            let scrub = scrub_pos(&events, ms, &cache, "A");
            match (playthrough, scrub) {
                (Some(playthrough_frame), Some(scrub_frame)) => assert!(
                    (playthrough_frame - scrub_frame).abs() < 1.0,
                    "t={ms}ms playthrough={playthrough_frame} scrub={scrub_frame} diff={}",
                    playthrough_frame - scrub_frame
                ),
                (None, None) => {}
                _ => panic!("t={ms}ms one path produced a deck and the other didn't"),
            }
        }
    }

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
        assert!(
            (before_play - 2.0 * SAMPLE_RATE_F64).abs() < 1.0,
            "pos={before_play}"
        );

        // play without sec resumes from the load position: 2s in + 2s played.
        let after_play = playthrough_pos(&events, 5000.0, &cache, "A").unwrap();
        assert!(
            (after_play - 4.0 * SAMPLE_RATE_F64).abs() < 1.0,
            "pos={after_play}"
        );

        assert_scrub_matches_playthrough(&events, &cache, 100, "A");
    }

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
        assert!((pos - 3.0 * SAMPLE_RATE_F64).abs() < 1.0, "pos={pos}");
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
        assert!((pos - 1.5 * SAMPLE_RATE_F64).abs() < 1.0, "pos={pos}");
        assert_scrub_matches_playthrough(&events, &cache, 100, "A");
    }

    #[test]
    fn filter_change_captured_in_post_event_snapshot() {
        let events = vec![
            SessionEvent::param(1000.0, Some("A"), "filter", "value", -0.5),
            SessionEvent::param(2000.0, Some("A"), "filter", "active", 1.0),
        ];
        let snaps = build_snapshots(&events, SAMPLE_RATE, &HashMap::new());
        let strip = snaps.last().unwrap().strips.get("A").unwrap();
        assert_eq!(strip.param("filter", "value").unwrap_or(0.0), -0.5);
        assert_eq!(strip.param("filter", "active"), Some(1.0));
    }

    #[test]
    fn filter_defaults_to_center_so_active_without_a_curve_is_transparent() {
        // Turning the filter on with no set_filter value must leave the knob at 0
        // (center/bypass), matching the timeline's DEFAULT_FILTER_VALUE. A stale
        // non-zero default would audibly filter where the drawn curve reads 0
        // (e.g. after stretching an active region back over an un-drawn stretch).
        let events = vec![SessionEvent::param(
            1000.0,
            Some("A"),
            "filter",
            "active",
            1.0,
        )];
        let snaps = build_snapshots(&events, SAMPLE_RATE, &HashMap::new());
        let strip = snaps.last().unwrap().strips.get("A").unwrap();
        assert_eq!(strip.param("filter", "value").unwrap_or(0.0), 0.0);
        assert_eq!(strip.param("filter", "active"), Some(1.0));
    }

    #[test]
    fn same_bucket_sub_ms_order_is_decided_by_raw_time() {
        let play = deck_ev("play", 100.2, "A");
        let stop = deck_ev("stop", 100.6, "A");
        assert_eq!(event_sim_order(&play, &stop), std::cmp::Ordering::Less);
        assert_eq!(event_sim_order(&stop, &play), std::cmp::Ordering::Greater);
    }

    #[test]
    fn exact_equal_ms_orders_enders_before_starters() {
        let play = deck_ev("play", 7000.0, "A");
        let stop = deck_ev("stop", 7000.0, "A");
        assert_eq!(event_sim_order(&stop, &play), std::cmp::Ordering::Less);
        assert_eq!(event_sim_order(&play, &stop), std::cmp::Ordering::Greater);
        let exit = deck_ev("exit_loop", 7000.0, "A");
        let loop_out = deck_ev("loop_out", 7000.0, "A");
        assert_eq!(event_sim_order(&exit, &loop_out), std::cmp::Ordering::Less);
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
        let snaps = build_snapshots(&events, SAMPLE_RATE, &cache);
        let deck = snaps.last().unwrap().decks.get("A").unwrap();
        assert_eq!(deck.bpm, Some(128.0));
        assert!((deck.beat_offset_frames - 0.25 * SAMPLE_RATE_F64).abs() < 1e-6);
    }
}
