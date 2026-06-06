// Rust tokio-based session playback scheduler.
// Replaces the JS setTimeout loop: one Tauri command starts a background task
// that applies events directly to AppState via tokio::time::sleep. No IPC per event.
//
// State preprocessing: on session load, `preload_session` simulates the entire
// event stream and captures full mixer+deck state every 500ms. Scrubbing to any
// position is then an O(log n) snapshot lookup + replay of at most 500ms of events.

use crate::audio;
use crate::offline_render::SessionEvent;
use crate::AppState;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub(crate) type SampleCache = HashMap<String, (Arc<Vec<f32>>, usize)>;

// ── Snapshot types ────────────────────────────────────────────────────────────

use crate::audio::DEFAULT_MASTER_GAIN;

// Internal simulation state. Not stored long-term.
#[derive(Clone)]
struct DeckSim {
    path: Option<String>,
    play_start_ms: f64,
    play_start_frame: f64,
    rate: f64,
    nudge_factor: f64,
    loop_active: bool,
    loop_start: f64,
    loop_end: f64,
    cue_point: f64,
    bpm: Option<f64>,
    beat_offset_frames: f64,
    total_frames: f64,
    is_playing: bool,
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
struct StripSim {
    gain: f32,
    eq_low: f32,
    eq_mid: f32,
    eq_high: f32,
    filter_value: f32,
    filter_active: bool,
}

impl Default for StripSim {
    fn default() -> Self {
        Self {
            gain: 1.0,
            eq_low: 0.0,
            eq_mid: 0.0,
            eq_high: 0.0,
            filter_value: 0.5,
            filter_active: false,
        }
    }
}

#[derive(Default)]
struct SimState {
    decks: HashMap<String, DeckSim>,
    strips: HashMap<String, StripSim>,
    master_gain: f32,
}

impl SimState {
    fn new() -> Self {
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
            filter_value: 0.5,
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

// ── Commands ──────────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub(crate) struct OpenedFile {
    path: String,
    content: String,
}

#[tauri::command]
pub async fn open_session_dialog() -> Option<OpenedFile> {
    let handle = rfd::AsyncFileDialog::new()
        .add_filter("Beatmatcher Session", &["bms"])
        .pick_file()
        .await?;
    let content = std::fs::read_to_string(handle.path()).ok()?;
    Some(OpenedFile {
        path: handle.path().to_string_lossy().into_owned(),
        content,
    })
}

#[tauri::command]
pub async fn preload_session(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let json = tokio::task::spawn_blocking({
        let p = path.clone();
        move || std::fs::read_to_string(&p).map_err(|e| format!("{p}: {e}"))
    })
    .await
    .map_err(|e| e.to_string())??;

    let session: crate::offline_render::SessionFile =
        serde_json::from_str(&json).map_err(|e| format!("parse error: {e}"))?;

    let sr = state.audio.device_sample_rate;
    let paths = session_track_paths(&session.events);
    populate_track_cache(&state, paths, sr).await;

    // Build state snapshots with the now-complete cache.
    let snapshots = {
        let cache = state
            .session_track_cache
            .lock()
            .expect("track cache mutex poisoned");
        build_snapshots(&session.events, sr, &cache)
    };

    state
        .session_snapshots
        .lock()
        .expect("snapshots mutex poisoned")
        .insert(path, snapshots);

    Ok(())
}

#[tauri::command]
pub async fn start_session_playback(
    state: tauri::State<'_, AppState>,
    path: String,
    from_ms: f64,
) -> Result<(), String> {
    let json = {
        let p = path.clone();
        tokio::task::spawn_blocking(move || {
            std::fs::read_to_string(&p).map_err(|e| format!("{p}: {e}"))
        })
        .await
        .map_err(|e| e.to_string())??
    };

    let session: crate::offline_render::SessionFile =
        serde_json::from_str(&json).map_err(|e| format!("parse error: {e}"))?;

    // Cancel any previous playback task.
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = state
            .session_playback_cancel
            .lock()
            .expect("session_playback mutex poisoned");
        if let Some(old) = guard.take() {
            old.store(true, Ordering::Release);
        }
        *guard = Some(cancel.clone());
    }

    let audio = state.audio.clone();
    let sr = audio.device_sample_rate;

    // Ensure cache is populated (fast path if preload already ran).
    let paths = session_track_paths(&session.events);
    populate_track_cache(&state, paths, sr).await;
    let cache: Arc<SampleCache> = Arc::new(
        state
            .session_track_cache
            .lock()
            .expect("track cache mutex poisoned")
            .clone(),
    );

    // Find the nearest snapshot at or before from_ms.
    let snapshot: Option<SessionSnapshot> = {
        let snaps = state
            .session_snapshots
            .lock()
            .expect("snapshots mutex poisoned");
        if let Some(snaps) = snaps.get(&path) {
            let idx = snaps.partition_point(|s| s.elapsed_ms <= from_ms);
            idx.checked_sub(1).map(|i| snaps[i].clone())
        } else {
            None
        }
    };

    tauri::async_runtime::spawn(async move {
        reset_all(&audio);

        let mut sorted_events = session.events;
        sorted_events.sort_by(|a, b| {
            a.elapsed_ms
                .partial_cmp(&b.elapsed_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Reconstruct state at from_ms: find the nearest post-event snapshot,
        // apply it, then replay any events that fall between the snapshot and from_ms.
        let (sim, snapshot_ms) = match snapshot {
            Some(ref snap) => (sim_state_from_snapshot(snap), snap.elapsed_ms),
            None => (SimState::new(), 0.0),
        };

        apply_sim_strips_and_master(&sim, &audio);

        let mut sim = sim;
        for ev in sorted_events
            .iter()
            .filter(|e| e.elapsed_ms > snapshot_ms && e.elapsed_ms <= from_ms)
        {
            match ev.event_type.as_str() {
                "set_volume" | "set_eq" | "set_filter" | "set_filter_active"
                | "set_master_gain" => {
                    apply_event_live(ev, &audio, sr, &cache);
                }
                _ => {}
            }
            sim_apply_event(ev, &mut sim, &cache, sr);
        }

        let sr_f = sr as f64;
        for (id, deck_sim) in &sim.decks {
            let pos = sim_pos(deck_sim, from_ms, sr_f);
            let Some(ref path) = deck_sim.path else {
                continue;
            };
            let Some((samples, channels)) = cache.get(path) else {
                continue;
            };
            let total_frames = samples.len() / channels;
            if let Some(deck_arc) = audio.deck(id) {
                let mut d = deck_arc.lock().expect("deck mutex poisoned");
                d.samples = samples.clone();
                d.channels = *channels;
                d.device_sample_rate = sr;
                d.total_frames = total_frames;
                d.duration = total_frames as f64 / sr as f64;
                d.loaded_path = Some(path.clone());
                d.main_pos = pos.min(total_frames as f64);
                d.cue_pos = d.main_pos;
                d.cue_point = deck_sim.cue_point.min(total_frames as f64);
                d.loop_active = deck_sim.loop_active;
                d.loop_end = deck_sim.loop_end.min(total_frames as f64);
                d.playback_rate = deck_sim.rate;
                d.nudge_factor = deck_sim.nudge_factor;
                d.bpm = deck_sim.bpm;
                d.beat_offset_frames = deck_sim.beat_offset_frames;
                d.is_playing = deck_sim.is_playing;
                d.is_cueing = false;
            }
        }

        let start = tokio::time::Instant::now();

        for event in sorted_events.iter().filter(|e| e.elapsed_ms > from_ms) {
            if cancel.load(Ordering::Acquire) {
                break;
            }

            let target =
                std::time::Duration::from_secs_f64((event.elapsed_ms - from_ms).max(0.0) / 1000.0);
            let elapsed = start.elapsed();
            if target > elapsed {
                tokio::time::sleep(target - elapsed).await;
            }

            if cancel.load(Ordering::Acquire) {
                break;
            }

            apply_event_live(event, &audio, sr, &cache);
        }

        if !cancel.load(Ordering::Acquire) {
            for id in ["A", "B", "C", "D"] {
                if let Some(deck_arc) = audio.deck(id) {
                    deck_arc.lock().expect("deck mutex poisoned").is_playing = false;
                }
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub fn stop_session_playback(state: tauri::State<'_, AppState>) {
    let mut guard = state
        .session_playback_cancel
        .lock()
        .expect("session_playback mutex poisoned");
    if let Some(cancel) = guard.take() {
        cancel.store(true, Ordering::Release);
    }
    for id in ["A", "B", "C", "D"] {
        if let Some(deck_arc) = state.audio.deck(id) {
            deck_arc.lock().expect("deck mutex poisoned").is_playing = false;
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn session_track_paths(events: &[SessionEvent]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    events
        .iter()
        .filter(|e| matches!(e.event_type.as_str(), "deck_snapshot" | "load_track"))
        .filter_map(|e| e.path.as_ref())
        .filter(|p| seen.insert(p.to_string()))
        .map(|p| p.to_string())
        .collect()
}

async fn populate_track_cache(state: &tauri::State<'_, AppState>, paths: Vec<String>, sr: u32) {
    let missing: Vec<String> = {
        let cache = state
            .session_track_cache
            .lock()
            .expect("track cache mutex poisoned");
        paths
            .into_iter()
            .filter(|p| !cache.contains_key(p))
            .collect()
    };

    let handles: Vec<_> = missing
        .iter()
        .map(|p| {
            let p = p.clone();
            tokio::task::spawn_blocking(move || -> Result<(String, Vec<f32>, usize), String> {
                let (raw, channels, native_sr) =
                    audio::decode_audio(&p).map_err(|e| e.to_string())?;
                let resampled = if native_sr == sr {
                    raw
                } else {
                    audio::resample_linear(&raw, channels, native_sr, sr)
                };
                Ok((p, resampled, channels))
            })
        })
        .collect();

    let mut newly_loaded = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(Ok((p, s, c))) => newly_loaded.push((p, s, c)),
            Ok(Err(e)) => eprintln!("session_playback: track load failed: {e}"),
            Err(e) => eprintln!("session_playback: spawn_blocking panic: {e}"),
        }
    }

    let mut cache = state
        .session_track_cache
        .lock()
        .expect("track cache mutex poisoned");
    for (p, samples, channels) in newly_loaded {
        cache.insert(p, (Arc::new(samples), channels));
    }
}

fn reset_all(audio: &audio::AppAudio) {
    for id in ["A", "B", "C", "D"] {
        if let Some(deck_arc) = audio.deck(id) {
            deck_arc.lock().expect("deck mutex poisoned").reset();
        }
        if let Some(strip_arc) = audio.strip(id) {
            strip_arc.lock().expect("strip mutex poisoned").reset();
        }
    }
    audio.monitor.set_master_gain(DEFAULT_MASTER_GAIN);
}

fn apply_sim_strips_and_master(sim: &SimState, audio: &audio::AppAudio) {
    audio.monitor.set_master_gain(sim.master_gain);
    for id in ["A", "B", "C", "D"] {
        let snap = sim.strips.get(id).cloned().unwrap_or_default();
        if let Some(strip_arc) = audio.strip(id) {
            let mut s = strip_arc.lock().expect("strip mutex poisoned");
            s.set_gain(snap.gain);
            s.set_eq_band("low", snap.eq_low);
            s.set_eq_band("mid", snap.eq_mid);
            s.set_eq_band("high", snap.eq_high);
            s.set_filter(snap.filter_value);
            s.set_filter_active(snap.filter_active);
        }
    }
}

fn sim_pos(sim: &DeckSim, at_ms: f64, sr_f: f64) -> f64 {
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

fn sim_state_from_snapshot(snap: &SessionSnapshot) -> SimState {
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

fn sim_apply_event(ev: &SessionEvent, state: &mut SimState, cache: &SampleCache, sr: u32) {
    let sr_f = sr as f64;
    let id = ev.deck.as_deref().unwrap_or("").to_string();

    match ev.event_type.as_str() {
        "deck_snapshot" | "load_track" => {
            let total_frames = ev
                .path
                .as_ref()
                .and_then(|p| cache.get(p))
                .map(|(s, c)| s.len() as f64 / *c as f64)
                .unwrap_or(0.0);
            let sim = state.decks.entry(id.clone()).or_default();
            sim.path = ev.path.clone();
            sim.total_frames = total_frames;
            sim.rate = ev.playback_rate.unwrap_or(1.0);
            if ev.event_type == "deck_snapshot" {
                let pos = ev.position_sec.unwrap_or(0.0) * sr_f;
                sim.play_start_frame = pos;
                sim.play_start_ms = 0.0;
                sim.is_playing = ev.is_playing.unwrap_or(false);
                sim.loop_active = ev.loop_active.unwrap_or(false);
                sim.loop_start = ev.cue_point_sec.map_or(0.0, |c| c * sr_f);
                sim.cue_point = sim.loop_start;
                sim.loop_end = ev.loop_end_sec.map_or(0.0, |e| e * sr_f);
                sim.bpm = ev.bpm;
            } else {
                let pos = ev.beat_offset_sec.unwrap_or(0.0) * sr_f;
                sim.play_start_frame = pos;
                sim.play_start_ms = ev.elapsed_ms;
                sim.is_playing = false;
                sim.loop_active = false;
                sim.cue_point = pos;
                sim.bpm = None;
                sim.beat_offset_frames = pos;
            }
        }
        "play" => {
            let sim = state.decks.entry(id).or_default();
            sim.play_start_frame = ev
                .sec
                .map(|s| s * sr_f)
                .unwrap_or_else(|| sim_pos(sim, ev.elapsed_ms, sr_f));
            sim.play_start_ms = ev.elapsed_ms;
            sim.is_playing = true;
        }
        "stop" => {
            let sim = state.decks.entry(id).or_default();
            sim.play_start_frame = sim_pos(sim, ev.elapsed_ms, sr_f);
            sim.play_start_ms = ev.elapsed_ms;
            sim.is_playing = false;
        }
        "stopped_at_cue" | "stop_at_cue" => {
            let sim = state.decks.entry(id).or_default();
            let pos = ev
                .cue_point_sec
                .map(|c| c * sr_f)
                .unwrap_or_else(|| sim_pos(sim, ev.elapsed_ms, sr_f));
            sim.play_start_frame = pos;
            sim.play_start_ms = ev.elapsed_ms;
            sim.is_playing = false;
        }
        "seek" => {
            if let Some(sec) = ev.sec {
                let sim = state.decks.entry(id).or_default();
                sim.play_start_frame = sec * sr_f;
                sim.play_start_ms = ev.elapsed_ms;
                sim.loop_active = sim.loop_active
                    && (sec * sr_f >= sim.loop_start)
                    && (sec * sr_f < sim.loop_end);
            }
        }
        "set_playback_rate" => {
            if let Some(r) = ev.rate {
                let sim = state.decks.entry(id).or_default();
                sim.play_start_frame = sim_pos(sim, ev.elapsed_ms, sr_f);
                sim.play_start_ms = ev.elapsed_ms;
                sim.rate = r.max(0.1);
            }
        }
        "set_nudge" => {
            if let Some(p) = ev.percent {
                let sim = state.decks.entry(id).or_default();
                sim.play_start_frame = sim_pos(sim, ev.elapsed_ms, sr_f);
                sim.play_start_ms = ev.elapsed_ms;
                sim.nudge_factor = 1.0 + p / 100.0;
            }
        }
        "loop_in" => {
            let sim = state.decks.entry(id).or_default();
            if let Some(cs) = ev.cue_sec {
                sim.loop_start = cs * sr_f;
                sim.cue_point = cs * sr_f;
            }
            sim.loop_end = 0.0;
            sim.loop_active = false;
        }
        "loop_out" => {
            let sim = state.decks.entry(id).or_default();
            if let Some(ss) = ev.start_sec {
                sim.loop_start = ss * sr_f;
                sim.cue_point = ss * sr_f;
            }
            if let Some(es) = ev.end_sec {
                sim.loop_end = es * sr_f;
            }
            sim.loop_active = true;
        }
        "exit_loop" => {
            state.decks.entry(id).or_default().loop_active = false;
        }
        "reloop" => {
            let sim = state.decks.entry(id).or_default();
            if sim.loop_end > sim.loop_start && sim.is_playing {
                sim.play_start_frame = sim.loop_start;
                sim.play_start_ms = ev.elapsed_ms;
                sim.loop_active = true;
            }
        }
        "eject_track" => {
            state.decks.remove(&id);
        }
        "set_volume" => {
            if let Some(g) = ev.gain {
                state.strips.entry(id).or_default().gain = g;
            }
        }
        "set_eq" => {
            if let (Some(ref band), Some(db)) = (&ev.band, ev.db) {
                let strip = state.strips.entry(id).or_default();
                match band.as_str() {
                    "low" => strip.eq_low = db,
                    "mid" => strip.eq_mid = db,
                    "high" => strip.eq_high = db,
                    _ => {}
                }
            }
        }
        "set_filter" => {
            if let Some(v) = ev.value {
                state.strips.entry(id).or_default().filter_value = v;
            }
        }
        "set_filter_active" => {
            if let Some(a) = ev.active {
                state.strips.entry(id).or_default().filter_active = a;
            }
        }
        "set_master_gain" => {
            if let Some(g) = ev.gain {
                state.master_gain = g;
            }
        }
        "cue_preview_start" => {
            let sim = state.decks.entry(id).or_default();
            let cp = ev.cue_point_sec.map(|c| c * sr_f).unwrap_or(sim.cue_point);
            sim.cue_point = cp;
            sim.play_start_frame = cp;
            sim.play_start_ms = ev.elapsed_ms;
            sim.is_playing = true;
        }
        "cue_preview_end" => {
            let sim = state.decks.entry(id).or_default();
            let cp = ev.cue_point_sec.map(|c| c * sr_f).unwrap_or(sim.cue_point);
            sim.play_start_frame = cp;
            sim.play_start_ms = ev.elapsed_ms;
            sim.is_playing = false;
        }
        _ => {}
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

// Build one snapshot per event, capturing state AFTER the event fires.
// Scrubbing to time T finds the last snapshot with elapsed_ms <= T and loads it.
// The exact post-event state, so no event is ever missing or double-applied.
// Events are sorted before simulation so the state machine progresses correctly
// regardless of order in the source JSON.
fn build_snapshots(events: &[SessionEvent], sr: u32, cache: &SampleCache) -> Vec<SessionSnapshot> {
    let sr_f = sr as f64;
    let mut state = SimState::new();
    let mut snapshots = Vec::new();

    let mut sorted: Vec<&SessionEvent> = events.iter().collect();
    sorted.sort_by(|a, b| {
        a.elapsed_ms
            .partial_cmp(&b.elapsed_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

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

// ── Live event application (used by the timer loop) ───────────────────────────

fn apply_event_live(ev: &SessionEvent, audio: &audio::AppAudio, sr: u32, cache: &SampleCache) {
    let sr_f = sr as f64;

    macro_rules! deck_arc {
        ($id:expr) => {
            match audio.deck($id) {
                Some(a) => a,
                None => return,
            }
        };
    }
    macro_rules! strip_arc {
        ($id:expr) => {
            match audio.strip($id) {
                Some(a) => a,
                None => return,
            }
        };
    }
    macro_rules! to_frames {
        ($sec:expr, $total:expr) => {
            ($sec * sr_f).clamp(0.0, $total as f64)
        };
    }

    match ev.event_type.as_str() {
        "deck_snapshot" | "load_track" => {
            let Some(ref id) = ev.deck else { return };
            let Some(ref path) = ev.path else { return };
            let Some((samples, channels)) = cache.get(path) else {
                eprintln!("session_playback: cache miss for {path}");
                return;
            };
            let total_frames = samples.len() / channels;
            let deck_a = deck_arc!(id.as_str());
            let mut d = deck_a.lock().expect("deck mutex poisoned");
            d.reset();
            d.samples = samples.clone();
            d.channels = *channels;
            d.device_sample_rate = sr;
            d.total_frames = total_frames;
            d.duration = total_frames as f64 / sr as f64;
            d.loaded_path = Some(path.clone());
            if ev.event_type == "deck_snapshot" {
                if let Some(pos) = ev.position_sec {
                    let f = to_frames!(pos, total_frames);
                    d.main_pos = f;
                    d.cue_pos = f;
                }
                if let Some(cp) = ev.cue_point_sec {
                    d.cue_point = to_frames!(cp, total_frames);
                }
                if let Some(bpm) = ev.bpm {
                    d.bpm = Some(bpm);
                }
                if let Some(rate) = ev.playback_rate {
                    d.playback_rate = rate.max(0.1);
                }
                if let Some(la) = ev.loop_active {
                    d.loop_active = la;
                }
                if let Some(le) = ev.loop_end_sec {
                    d.loop_end = to_frames!(le, total_frames);
                }
                if ev.is_playing == Some(true) {
                    d.is_playing = true;
                }
            } else if let Some(offset) = ev.beat_offset_sec {
                let f = to_frames!(offset, total_frames);
                d.main_pos = f;
                d.cue_pos = f;
                d.cue_point = f;
            }
        }

        "eject_track" => {
            let Some(ref id) = ev.deck else { return };
            deck_arc!(id.as_str())
                .lock()
                .expect("deck mutex poisoned")
                .eject();
        }

        "play" => {
            let Some(ref id) = ev.deck else { return };
            let deck_a = deck_arc!(id.as_str());
            let mut d = deck_a.lock().expect("deck mutex poisoned");
            if let Some(sec) = ev.sec {
                let f = to_frames!(sec, d.total_frames);
                d.main_pos = f;
                d.cue_pos = f;
            } else {
                d.cue_pos = d.main_pos;
            }
            d.is_playing = true;
        }

        "stop" => {
            let Some(ref id) = ev.deck else { return };
            deck_arc!(id.as_str())
                .lock()
                .expect("deck mutex poisoned")
                .is_playing = false;
        }

        "stopped_at_cue" | "stop_at_cue" => {
            let Some(ref id) = ev.deck else { return };
            let deck_a = deck_arc!(id.as_str());
            let mut d = deck_a.lock().expect("deck mutex poisoned");
            d.is_playing = false;
            if let Some(cp) = ev.cue_point_sec {
                let f = to_frames!(cp, d.total_frames);
                d.main_pos = f;
                d.cue_pos = f;
            }
        }

        "seek" => {
            let Some(ref id) = ev.deck else { return };
            if let Some(sec) = ev.sec {
                let deck_a = deck_arc!(id.as_str());
                let mut d = deck_a.lock().expect("deck mutex poisoned");
                let f = to_frames!(sec, d.total_frames);
                d.main_pos = f;
                d.cue_pos = f;
                if d.loop_active && (f < d.cue_point || f >= d.loop_end) {
                    d.loop_active = false;
                }
            }
        }

        "set_volume" => {
            let Some(ref id) = ev.deck else { return };
            if let Some(g) = ev.gain {
                strip_arc!(id.as_str())
                    .lock()
                    .expect("strip mutex poisoned")
                    .set_gain(g);
            }
        }

        "set_eq" => {
            let Some(ref id) = ev.deck else { return };
            if let (Some(ref band), Some(db)) = (&ev.band, ev.db) {
                strip_arc!(id.as_str())
                    .lock()
                    .expect("strip mutex poisoned")
                    .set_eq_band(band, db);
            }
        }

        "set_filter" => {
            let Some(ref id) = ev.deck else { return };
            if let Some(v) = ev.value {
                strip_arc!(id.as_str())
                    .lock()
                    .expect("strip mutex poisoned")
                    .set_filter(v);
            }
        }

        "set_filter_active" => {
            let Some(ref id) = ev.deck else { return };
            if let Some(a) = ev.active {
                strip_arc!(id.as_str())
                    .lock()
                    .expect("strip mutex poisoned")
                    .set_filter_active(a);
            }
        }

        "set_playback_rate" => {
            let Some(ref id) = ev.deck else { return };
            if let Some(r) = ev.rate {
                deck_arc!(id.as_str())
                    .lock()
                    .expect("deck mutex poisoned")
                    .playback_rate = r.max(0.1);
            }
        }

        "set_nudge" => {
            let Some(ref id) = ev.deck else { return };
            if let Some(p) = ev.percent {
                deck_arc!(id.as_str())
                    .lock()
                    .expect("deck mutex poisoned")
                    .nudge_factor = 1.0 + p / 100.0;
            }
        }

        "set_master_gain" => {
            if let Some(g) = ev.gain {
                audio.monitor.set_master_gain(g);
            }
        }

        "set_beat_grid" => {
            let Some(ref id) = ev.deck else { return };
            let deck_a = deck_arc!(id.as_str());
            let mut d = deck_a.lock().expect("deck mutex poisoned");
            if let Some(bpm) = ev.bpm {
                d.bpm = Some(bpm);
            }
            if let Some(off) = ev.beat_offset_sec {
                d.beat_offset_frames = off * sr_f;
            }
        }

        "loop_in" => {
            let Some(ref id) = ev.deck else { return };
            let deck_a = deck_arc!(id.as_str());
            let mut d = deck_a.lock().expect("deck mutex poisoned");
            if let Some(cs) = ev.cue_sec {
                d.cue_point = to_frames!(cs, d.total_frames);
            }
            d.loop_active = false;
            d.loop_end = 0.0;
        }

        "loop_out" => {
            let Some(ref id) = ev.deck else { return };
            let deck_a = deck_arc!(id.as_str());
            let mut d = deck_a.lock().expect("deck mutex poisoned");
            if let Some(ss) = ev.start_sec {
                d.cue_point = to_frames!(ss, d.total_frames);
            }
            if let Some(es) = ev.end_sec {
                d.loop_end = to_frames!(es, d.total_frames);
            }
            d.loop_active = true;
        }

        "exit_loop" => {
            let Some(ref id) = ev.deck else { return };
            deck_arc!(id.as_str())
                .lock()
                .expect("deck mutex poisoned")
                .loop_active = false;
        }

        "reloop" => {
            let Some(ref id) = ev.deck else { return };
            let deck_a = deck_arc!(id.as_str());
            let mut d = deck_a.lock().expect("deck mutex poisoned");
            if d.loop_end > d.cue_point {
                d.main_pos = d.cue_point;
                d.cue_pos = d.cue_point;
                if d.is_playing {
                    d.loop_active = true;
                }
            }
        }

        "cue_preview_start" => {
            let Some(ref id) = ev.deck else { return };
            let deck_a = deck_arc!(id.as_str());
            let mut d = deck_a.lock().expect("deck mutex poisoned");
            let cp = ev.cue_point_sec.unwrap_or(d.cue_point / sr_f);
            let f = to_frames!(cp, d.total_frames);
            d.cue_point = f;
            d.main_pos = f;
            d.cue_pos = f;
            d.is_playing = true;
            d.is_cueing = true;
        }

        "cue_preview_end" => {
            let Some(ref id) = ev.deck else { return };
            let deck_a = deck_arc!(id.as_str());
            let mut d = deck_a.lock().expect("deck mutex poisoned");
            let cp = ev.cue_point_sec.unwrap_or(d.cue_point / sr_f);
            let f = to_frames!(cp, d.total_frames);
            d.is_playing = false;
            d.is_cueing = false;
            d.main_pos = f;
            d.cue_pos = f;
        }

        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offline_render::SessionEvent;

    const SR: u32 = 44100;
    const SR_F: f64 = 44100.0;

    fn deck_ev(event_type: &str, elapsed_ms: f64, deck: &str) -> SessionEvent {
        SessionEvent {
            event_type: event_type.to_string(),
            elapsed_ms,
            deck: Some(deck.to_string()),
            ..Default::default()
        }
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
}
