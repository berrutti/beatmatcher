use crate::audio;
use crate::lock::LockIgnoringPoison;
use session_core::event::SessionCommand;
use session_core::event::SessionEvent;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::Emitter;

use crate::audio::DEFAULT_MASTER_GAIN;

// The deterministic simulation (state machine, position math, snapshots) lives
// in the shared `session-core` crate so the engine and the frontend can never
// disagree. This module wires it to the real audio engine.
use session_core::{
    build_snapshots, event_sim_order, sim_apply_event, sim_pos, sim_state_from_snapshot, SimState,
};
pub(crate) use session_core::{SampleCache, SessionSnapshot};

#[derive(serde::Serialize)]
pub(crate) struct OpenedFile {
    path: String,
    content: String,
}

async fn index_session(
    sessions: &SessionLibrary,
    engine: &crate::engine::Engine,
    path: String,
    session: session_core::event::SessionFile,
    app: Option<&tauri::AppHandle>,
) {
    let sr = engine.audio.device_sample_rate;
    let paths = session_track_paths(&session.events);
    let reporter = app.map(|app| LoadReporter {
        app,
        path: path.clone(),
    });
    populate_track_cache(sessions, paths, sr, reporter.as_ref()).await;

    if let Some(reporter) = &reporter {
        reporter.phase("indexing");
    }

    let snapshots = {
        let cache = sessions.track_cache.locked();
        build_snapshots(&session.events, sr, &cache)
    };

    sessions.snapshots.locked().insert(path.clone(), snapshots);
    sessions
        .files
        .locked()
        .insert(path.clone(), Arc::new(session));

    // Only here: scrubbing and playback both need the snapshots, so a `done`
    // sent when the last track finished decoding would open the gate early.
    if let Some(reporter) = &reporter {
        reporter.emit(SessionLoadProgress {
            path,
            phase: "done",
            loaded_bytes: 0,
            total_bytes: 0,
            loaded_tracks: 0,
            total_tracks: 0,
            done: true,
        });
    }
}

fn session_track_paths(events: &[SessionEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e.command() {
            Some(
                SessionCommand::DeckSnapshot { path, .. } | SessionCommand::LoadTrack { path, .. },
            ) => Some(path.to_string()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub struct SessionLibrary {
    pub playback_cancel: std::sync::Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>,
    pub playback_handle: std::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    pub track_cache: Arc<std::sync::Mutex<SampleCache>>,
    pub track_loads: TrackLoads,
    pub decode_permits: Arc<tokio::sync::Semaphore>,
    pub snapshots: std::sync::Mutex<std::collections::HashMap<String, Vec<SessionSnapshot>>>,
    // Holds unsaved edits pushed from the frontend, so playback and the offline render
    // are audible before the file is written.
    pub files:
        std::sync::Mutex<std::collections::HashMap<String, Arc<session_core::event::SessionFile>>>,
}

impl SessionLibrary {
    pub fn new() -> Self {
        Self {
            playback_cancel: std::sync::Mutex::new(None),
            playback_handle: std::sync::Mutex::new(None),
            track_cache: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            track_loads: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            // Bounds every decode in the app: throttling only the preload made it lose
            // the race against unbounded waveform requests.
            decode_permits: Arc::new(tokio::sync::Semaphore::new(
                std::thread::available_parallelism()
                    .map(|cores| cores.get())
                    .unwrap_or(4),
            )),
            snapshots: std::sync::Mutex::new(std::collections::HashMap::new()),
            files: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

pub(crate) type TrackEntry = (Arc<Vec<f32>>, usize);
pub(crate) type TrackLoads = Arc<
    std::sync::Mutex<
        std::collections::HashMap<String, Arc<tokio::sync::OnceCell<Option<TrackEntry>>>>,
    >,
>;

/// The one place a track is decoded. The waveform strip and the session preload want the
/// same samples at once, and decoding per caller made 13 tracks into 26 competing decodes.
pub(crate) async fn load_track(
    cache: &Arc<std::sync::Mutex<SampleCache>>,
    loads: &TrackLoads,
    permits: &Arc<tokio::sync::Semaphore>,
    path: &str,
    sr: u32,
) -> Option<TrackEntry> {
    if let Some(hit) = cache.locked().get(path).cloned() {
        return Some(hit);
    }

    // Only whoever installed the cell takes it out again. A waiter removing it
    // would drop a cell another caller had already replaced, letting a third
    // decode start for the same path.
    let (cell, owns_cell) = {
        let mut loads = loads.locked();
        match loads.get(path) {
            Some(existing) => (existing.clone(), false),
            None => {
                let fresh = Arc::new(tokio::sync::OnceCell::new());
                loads.insert(path.to_string(), fresh.clone());
                (fresh, true)
            }
        }
    };

    let entry = cell
        .get_or_init(|| async {
            let permit = permits.clone().acquire_owned().await;
            let owned = path.to_string();
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                let (raw, channels, native_sr) = audio::decode_audio(&owned).ok()?;
                let resampled = if native_sr == sr {
                    raw
                } else {
                    audio::resample_linear(&raw, channels, native_sr, sr)
                };
                Some((Arc::new(resampled), channels))
            })
            .await
            .ok()
            .flatten()
        })
        .await
        .clone();

    if let Some(entry) = &entry {
        cache.locked().insert(path.to_string(), entry.clone());
    }
    if owns_cell {
        loads.locked().remove(path);
    }
    entry
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionLoadProgress {
    pub path: String,
    pub phase: &'static str,
    pub loaded_bytes: u64,
    pub total_bytes: u64,
    pub loaded_tracks: usize,
    pub total_tracks: usize,
    pub done: bool,
}

struct LoadReporter<'a> {
    app: &'a tauri::AppHandle,
    path: String,
}

impl LoadReporter<'_> {
    fn emit(&self, progress: SessionLoadProgress) {
        self.app.emit("session-load-progress", progress).ok();
    }

    // The read and the parse each take seconds on a long session and report no
    // increments, so they are announced rather than left as a still bar.
    fn phase(&self, phase: &'static str) {
        self.emit(SessionLoadProgress {
            path: self.path.clone(),
            phase,
            loaded_bytes: 0,
            total_bytes: 0,
            loaded_tracks: 0,
            total_tracks: 0,
            done: false,
        });
    }
}

async fn populate_track_cache(
    sessions: &SessionLibrary,
    paths: Vec<String>,
    sr: u32,
    reporter: Option<&LoadReporter<'_>>,
) {
    let missing: Vec<String> = {
        let cache = sessions.track_cache.locked();
        paths
            .into_iter()
            .filter(|p| !cache.contains_key(p))
            .collect()
    };

    // Byte-weighted so one long track and three short ones does not sit at 25% for most of
    // the wait, and smallest-first so the earliest slots free soonest.
    let mut sized: Vec<(String, u64)> = missing
        .into_iter()
        .map(|p| {
            let bytes = std::fs::metadata(&p).map(|meta| meta.len()).unwrap_or(0);
            (p, bytes)
        })
        .collect();
    sized.sort_by_key(|(_, bytes)| *bytes);

    let total_bytes: u64 = sized.iter().map(|(_, bytes)| bytes).sum();
    let total_tracks = sized.len();
    let mut loaded_bytes: u64 = 0;
    let mut loaded_tracks: usize = 0;

    if let Some(reporter) = &reporter {
        reporter.emit(SessionLoadProgress {
            path: reporter.path.clone(),
            phase: "decoding",
            loaded_bytes: 0,
            total_bytes,
            loaded_tracks: 0,
            total_tracks,
            done: false,
        });
    }

    // Every decode in the app shares one permit pool, so a burst of waveform
    // requests cannot crowd out the load the modal is reporting on.
    let handles: Vec<_> = sized
        .iter()
        .map(|(p, _)| {
            let path = p.clone();
            let cache = sessions.track_cache.clone();
            let loads = sessions.track_loads.clone();
            let permits = sessions.decode_permits.clone();
            tokio::spawn(async move {
                load_track(&cache, &loads, &permits, &path, sr)
                    .await
                    .is_some()
            })
        })
        .collect();

    for (handle, (path, bytes)) in handles.into_iter().zip(sized.iter()) {
        // Counted even when it fails: a track that cannot be decoded is never
        // coming, and a bar that stops short of 100% reads as a hang.
        loaded_bytes += bytes;
        if !matches!(handle.await, Ok(true)) {
            eprintln!("session_playback: track load failed: {path}");
        }
        loaded_tracks += 1;
        if let Some(reporter) = &reporter {
            reporter.emit(SessionLoadProgress {
                path: reporter.path.clone(),
                phase: "decoding",
                loaded_bytes,
                total_bytes,
                loaded_tracks,
                total_tracks,
                done: false,
            });
        }
    }
}

/// Mixer state a mid-session start rebuilds by replaying the events before it. Transport
/// is excluded, since deck positions come from the snapshot and `sim_pos`.
fn reconstructs_mixer_state(event: &SessionEvent) -> bool {
    matches!(
        event.command(),
        Some(
            SessionCommand::SetParam { .. }
                | SessionCommand::SetXfaderAssign { .. }
                | SessionCommand::SetJogRotationSpeed { .. }
        )
    )
}

fn reset_all(audio: &audio::AppAudio) {
    for id in crate::audio::LIVE_DECK_IDS {
        if let Some(deck_arc) = audio.deck(id) {
            deck_arc.locked().reset();
        }
        if let Some(strip_arc) = audio.strip(id) {
            strip_arc.locked().reset();
        }
    }
    audio.monitor.set_master_gain(DEFAULT_MASTER_GAIN);
}

fn apply_sim_strips_and_master(sim: &SimState, audio: &audio::AppAudio) {
    audio.monitor.set_master_gain(sim.master_gain);
    for id in crate::audio::LIVE_DECK_IDS {
        let snap = sim.strips.get(id).cloned().unwrap_or_default();
        if let Some(strip_arc) = audio.strip(id) {
            let mut s = strip_arc.locked();
            for (address, value) in &snap.params {
                if let Some((slot, param)) = address.split_once('/') {
                    s.set_param(slot, param, *value);
                }
            }
            s.set_xfader_assign(snap.xfader_assign);
            s.set_xfader_position(sim.xfader_position);
            s.set_fader_curve(sim.fader_curve);
        }
        if let Some(deck_arc) = audio.deck(id) {
            deck_arc
                .locked()
                .set_jog_rotation_speed(sim.jog_rotation_speed);
        }
    }
}

async fn wait_until_frame(
    monitor: &audio::MasterMonitor,
    target_frame: u64,
    sr: u32,
    cancel: &AtomicBool,
) {
    let sr_f = sr.max(1) as f64;
    let mut last_seen = monitor.output_frames();
    let mut last_change = std::time::Instant::now();

    loop {
        if cancel.load(Ordering::Acquire) {
            return;
        }
        let now = monitor.output_frames();
        if now >= target_frame {
            return;
        }
        if now != last_seen {
            last_seen = now;
            last_change = std::time::Instant::now();
        } else if last_change.elapsed() > std::time::Duration::from_millis(500) {
            return;
        }
        let remaining = target_frame - now;
        let secs = remaining as f64 / sr_f;
        // Cap the step so a pending cancel (e.g. a new scrub waiting on this task
        // to exit) is noticed within ~10ms instead of up to a full step.
        tokio::time::sleep(std::time::Duration::from_secs_f64(secs.clamp(0.0005, 0.01))).await;
    }
}

// An event lands up to one buffer late, so a deck it starts or repositions is advanced by
// `overshoot_frames * rate` to sit level with decks already playing. Pass 0 to skip that.
fn apply_event_live(
    event: &SessionEvent,
    audio: &audio::AppAudio,
    sr: u32,
    cache: &SampleCache,
    overshoot_frames: u64,
) {
    let overshoot_f = overshoot_frames as f64;

    let Some(cmd) = event.command() else { return };

    let Some(id) = cmd.deck_id() else {
        match cmd {
            SessionCommand::SetParam {
                scope: session_core::ParamScope::Master,
                slot: "gain",
                param: "gain",
                value,
                ..
            } => audio.monitor.set_master_gain(value as f32),
            SessionCommand::SetParam {
                scope: session_core::ParamScope::Master,
                slot: "xfader",
                param: "position",
                value,
                ..
            } => {
                for id in crate::audio::LIVE_DECK_IDS {
                    if let Some(strip_arc) = audio.strip(id) {
                        strip_arc.locked().set_xfader_position(value as f32);
                    }
                }
            }
            SessionCommand::SetFaderCurve { curve } => {
                for id in crate::audio::LIVE_DECK_IDS {
                    if let Some(strip_arc) = audio.strip(id) {
                        strip_arc.locked().set_fader_curve(curve);
                    }
                }
            }
            SessionCommand::SetJogRotationSpeed { speed } => {
                for id in crate::audio::LIVE_DECK_IDS {
                    if let Some(deck_arc) = audio.deck(id) {
                        deck_arc.locked().set_jog_rotation_speed(speed);
                    }
                }
            }
            _ => {}
        }
        return;
    };

    let (Some(deck_a), Some(strip_a)) = (audio.deck(id), audio.strip(id)) else {
        return;
    };
    let mut d = deck_a.locked();
    let mut s = strip_a.locked();

    let mut load_samples = |path: &str| -> Result<(Arc<Vec<f32>>, usize), String> {
        cache
            .get(path)
            .map(|(samples, channels)| (samples.clone(), *channels))
            .ok_or_else(|| format!("cache miss for {path}"))
    };

    if let Err(e) =
        audio::apply_deck_command(&cmd, &mut d, &mut s, sr, overshoot_f, &mut load_samples)
    {
        eprintln!("session_playback: {e}");
    }
}

pub(crate) async fn open_session_dialog() -> Option<crate::session_playback::OpenedFile> {
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

pub(crate) async fn preload_session(
    app: tauri::AppHandle,
    engine: tauri::State<'_, crate::engine::Engine>,
    sessions: tauri::State<'_, crate::session_playback::SessionLibrary>,
    path: String,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    let reporter = LoadReporter {
        app: &app,
        path: path.clone(),
    };
    reporter.phase("reading");

    let json = tokio::task::spawn_blocking({
        let p = path.clone();
        move || std::fs::read_to_string(&p).map_err(|e| format!("{p}: {e}"))
    })
    .await
    .map_err(|e| e.to_string())??;

    reporter.phase("parsing");
    // Off the async thread: a long session's event array takes seconds to deserialize, and
    // blocking here stalls every other command including this load's own progress events.
    let session = tokio::task::spawn_blocking(move || {
        session_core::event::SessionFile::parse(&json).map_err(|e| format!("parse error: {e}"))
    })
    .await
    .map_err(|e| e.to_string())??;

    index_session(&sessions, &engine, path, session, Some(&app)).await;
    log::info!(
        "preload_session: ready in {}ms",
        started.elapsed().as_millis()
    );

    Ok(())
}

// Frees everything cached for a session when it is ejected: decoded track
// samples are the bulk of it (hundreds of MB for a multi-track session).
// Playback of a path that is no longer cached falls back to a disk read.
pub(crate) fn unload_session(
    sessions: tauri::State<'_, crate::session_playback::SessionLibrary>,
    path: String,
) {
    let removed = sessions.files.locked().remove(&path);
    sessions.snapshots.locked().remove(&path);
    if let Some(session) = removed {
        let track_paths = session_track_paths(&session.events);
        let mut cache = sessions.track_cache.locked();
        for track_path in track_paths {
            cache.remove(&track_path);
        }
    }
}

// Replaces the in-memory event list for a loaded session with edited events
// from the frontend. The .bms on disk is untouched. The next playback, scrub,
// or render uses the edited events.
pub(crate) async fn update_session_events(
    engine: tauri::State<'_, crate::engine::Engine>,
    sessions: tauri::State<'_, crate::session_playback::SessionLibrary>,
    path: String,
    events_json: String,
) -> Result<(), String> {
    let events: Vec<SessionEvent> =
        serde_json::from_str(&events_json).map_err(|e| format!("parse error: {e}"))?;

    // Editing events does not change which mixer the session was played on, so
    // the header carries over from the file that was loaded.
    let mixer = sessions
        .files
        .locked()
        .get(&path)
        .and_then(|session| session.mixer.clone());

    index_session(
        &sessions,
        &engine,
        path,
        // Current, not carried over: loading ported these events forward.
        session_core::event::SessionFile {
            version: session_core::BMS_VERSION,
            events,
            mixer,
        },
        None,
    )
    .await;

    Ok(())
}

pub(crate) async fn start_session_playback(
    app: tauri::AppHandle,
    engine: tauri::State<'_, crate::engine::Engine>,
    sessions: tauri::State<'_, crate::session_playback::SessionLibrary>,
    path: String,
    from_ms: f64,
) -> Result<(), String> {
    // Prefer the in-memory session (which may hold unsaved edits). Fall back to
    // the disk file for callers that never preloaded.
    let cached: Option<Arc<session_core::event::SessionFile>> =
        sessions.files.locked().get(&path).cloned();
    let session: Arc<session_core::event::SessionFile> = match cached {
        Some(cached_session) => cached_session,
        None => {
            let json = {
                let p = path.clone();
                tokio::task::spawn_blocking(move || {
                    std::fs::read_to_string(&p).map_err(|e| format!("{p}: {e}"))
                })
                .await
                .map_err(|e| e.to_string())??
            };
            let parsed = session_core::event::SessionFile::parse(&json)
                .map_err(|e| format!("parse error: {e}"))?;
            let parsed = Arc::new(parsed);
            sessions.files.locked().insert(path.clone(), parsed.clone());
            parsed
        }
    };

    // Awaited, not just cancelled: the runtime is multi-threaded, so the old task can
    // otherwise apply a stale event after the new one has already placed that deck.
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = sessions.playback_cancel.locked();
        if let Some(old) = guard.take() {
            old.store(true, Ordering::Release);
        }
        *guard = Some(cancel.clone());
    }
    let old_handle = sessions.playback_handle.locked().take();
    if let Some(handle) = old_handle {
        let _ = handle.await;
    }

    // Refused rather than replayed against the wrong scales, which would differ from
    // the offline render. Hosting beats an id match, so pre-versioned sessions play.
    let manifest = session_core::resolve_manifest(session.mixer.as_ref())?;
    if !engine.audio.mixer().can_host(manifest) {
        return Err(format!(
            "this session was recorded on mixer '{}', which this build cannot play live",
            manifest.id
        ));
    }

    let audio = engine.audio.clone();
    let sr = audio.device_sample_rate;

    // Ensure cache is populated (fast path if preload already ran).
    let paths = session_track_paths(&session.events);
    populate_track_cache(&sessions, paths, sr, None).await;
    let cache: Arc<SampleCache> = Arc::new(sessions.track_cache.locked().clone());

    // Find the nearest snapshot at or before from_ms.
    let snapshot: Option<SessionSnapshot> = {
        let snaps = sessions.snapshots.locked();
        if let Some(snaps) = snaps.get(&path) {
            let idx = snaps.partition_point(|s| s.elapsed_ms <= from_ms);
            idx.checked_sub(1).map(|i| snaps[i].clone())
        } else {
            None
        }
    };

    let app_handle = app.clone();
    let handle = tauri::async_runtime::spawn(async move {
        reset_all(&audio);

        let mut sorted_events = session.events.clone();
        sorted_events.sort_by(event_sim_order);

        // Reconstruct state at from_ms: find the nearest post-event snapshot,
        // apply it, then replay any events that fall between the snapshot and from_ms.
        let (sim, snapshot_ms) = match snapshot {
            Some(ref snap) => (sim_state_from_snapshot(snap), snap.elapsed_ms),
            None => (SimState::new(), 0.0),
        };

        apply_sim_strips_and_master(&sim, &audio);

        let mut sim = sim;
        // deck_snapshot is already folded into the base snapshot. Never replay it.
        for event in sorted_events.iter().filter(|e| {
            e.elapsed_ms > snapshot_ms && e.elapsed_ms <= from_ms && e.event_type != "deck_snapshot"
        }) {
            if reconstructs_mixer_state(event) {
                apply_event_live(event, &audio, sr, &cache, 0);
            }
            sim_apply_event(event, &mut sim, &cache, sr);
        }

        let sr_f = sr.max(1) as f64;

        // Two passes so every deck begins on the same output buffer: setup under brief
        // individual locks with is_playing false, then the flags under all locks at once.
        let mut ids: Vec<&String> = sim.decks.keys().collect();
        ids.sort();
        let mut to_start: Vec<Arc<std::sync::Mutex<audio::Deck>>> = Vec::new();
        for id in ids {
            let ds = &sim.decks[id];
            let (Some(path), Some(arc)) = (ds.path.as_ref(), audio.deck(id)) else {
                continue;
            };
            let Some((samples, channels)) = cache.get(path) else {
                continue;
            };
            {
                let mut d = arc.locked();
                d.load(path, samples.clone(), *channels, sr);
                d.restore(audio::DeckRestore {
                    position: sim_pos(ds, from_ms, sr_f),
                    cue_point: ds.cue_point,
                    loop_active: ds.loop_active,
                    loop_end: ds.loop_end,
                    playback_rate: ds.rate,
                    jog_hold_factor: ds.jog_hold_factor,
                    bpm: ds.bpm,
                    beat_offset_frames: ds.beat_offset_frames,
                });
            }
            if ds.is_playing {
                to_start.push(arc);
            }
        }
        {
            let mut guards: Vec<_> = to_start.iter().map(|a| a.locked()).collect();
            for d in guards.iter_mut() {
                d.is_playing = true;
            }
        }

        // The OS clock and the soundcard clock are different oscillators, so waiting on
        // wall time turns a one-time error into a permanent phase offset.
        let base_frame = audio.monitor.output_frames();

        for event in sorted_events.iter().filter(|e| e.elapsed_ms > from_ms) {
            if cancel.load(Ordering::Acquire) {
                break;
            }

            let target_offset = ((event.elapsed_ms - from_ms).max(0.0) / 1000.0 * sr_f).round();
            let target_frame = base_frame.saturating_add(target_offset as u64);
            wait_until_frame(&audio.monitor, target_frame, sr, &cancel).await;

            if cancel.load(Ordering::Acquire) {
                break;
            }

            // How far the audio clock is already past this event's target frame
            // (the event only takes effect on the next callback). Used to
            // sample-align a deck that starts/repositions here with decks that
            // were already playing.
            let overshoot = audio.monitor.output_frames().saturating_sub(target_frame);
            apply_event_live(event, &audio, sr, &cache, overshoot);
        }

        if !cancel.load(Ordering::Acquire) {
            for id in crate::audio::LIVE_DECK_IDS {
                if let Some(deck_arc) = audio.deck(id) {
                    deck_arc.locked().stop();
                }
            }
            app_handle.emit("session-playback-ended", ()).ok();
        }
    });

    *sessions.playback_handle.locked() = Some(handle);

    Ok(())
}

pub(crate) fn stop_session_playback(
    engine: tauri::State<'_, crate::engine::Engine>,
    sessions: tauri::State<'_, crate::session_playback::SessionLibrary>,
) {
    let mut guard = sessions.playback_cancel.locked();
    if let Some(cancel) = guard.take() {
        cancel.store(true, Ordering::Release);
    }
    for id in crate::audio::LIVE_DECK_IDS {
        if let Some(deck_arc) = engine.audio.deck(id) {
            deck_arc.locked().stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{ChannelStrip, Deck};
    use std::collections::{HashMap, HashSet};

    const SR: u32 = 44100;
    const SR_F: f64 = 44100.0;

    #[test]
    fn a_mid_session_start_rebuilds_assigns_as_well_as_params() {
        let assign = SessionEvent {
            assign: Some("a".to_string()),
            ..deck_ev("set_xfader_assign", 10_000.0, "A")
        };
        let param = SessionEvent::param(11_000.0, Some("A"), "fader", "gain", 0.5);
        let position = SessionEvent::param(12_000.0, None, "xfader", "position", -1.0);

        assert!(reconstructs_mixer_state(&assign));
        assert!(reconstructs_mixer_state(&param));
        assert!(reconstructs_mixer_state(&position));
        assert!(!reconstructs_mixer_state(&deck_ev("play", 13_000.0, "A")));
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

    // A full loop life cycle: loop_in → loop_out → wrap → exit_loop → reloop →
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

    // The scrub placement the live scheduler computes: apply every event up to
    // from_ms through the shared sim, then read sim_pos.
    fn playthrough_pos(
        events: &[SessionEvent],
        from_ms: f64,
        cache: &SampleCache,
        deck: &str,
    ) -> Option<f64> {
        let mut sorted: Vec<&SessionEvent> = events.iter().collect();
        sorted.sort_by(|a, b| a.elapsed_ms.partial_cmp(&b.elapsed_ms).unwrap());
        let mut state = SimState::new();
        for event in sorted.iter().filter(|e| e.elapsed_ms <= from_ms) {
            sim_apply_event(event, &mut state, cache, SR);
        }
        state.decks.get(deck).map(|d| sim_pos(d, from_ms, SR_F))
    }

    // Must go through the production applier: a private reimplementation could
    // pass the parity check while production diverges.
    fn apply_deck_event(
        d: &mut Deck,
        s: &mut ChannelStrip,
        event: &SessionEvent,
        cache: &SampleCache,
    ) {
        let Some(cmd) = event.command() else { return };
        if cmd.deck_id().is_none() {
            return;
        }
        let mut load_samples = |path: &str| -> Result<(Arc<Vec<f32>>, usize), String> {
            cache
                .get(path)
                .map(|(samples, channels)| (samples.clone(), *channels))
                .ok_or_else(|| format!("cache miss for {path}"))
        };
        audio::apply_deck_command(&cmd, d, s, SR, 0.0, &mut load_samples)
            .expect("apply_deck_command failed in parity test");
    }

    fn param_ev(elapsed_ms: f64, slot: &str, param: &str, value: f32) -> SessionEvent {
        SessionEvent {
            slot: Some(slot.to_string()),
            param: Some(param.to_string()),
            value: Some(value),
            ..deck_ev("set_param", elapsed_ms, "A")
        }
    }

    #[test]
    fn an_unknown_slot_or_param_is_skipped_and_the_rest_still_applies() {
        let events = [
            param_ev(0.0, "resonator", "drive", 0.9),
            param_ev(10.0, "eq", "sub", 1.0),
            param_ev(20.0, "fader", "gain", 0.25),
        ];

        let cache: SampleCache = HashMap::new();
        let mut deck = Deck::empty(SR);
        let mut strip = ChannelStrip::new(SR_F as f32);
        let mut sim = SimState::new();
        for event in &events {
            apply_deck_event(&mut deck, &mut strip, event, &cache);
            sim_apply_event(event, &mut sim, &cache, SR);
        }

        assert!(
            (strip.target_gain() - 0.25).abs() < 1e-6,
            "engine stopped at the unknown params, gain is {}",
            strip.target_gain()
        );
        assert!(
            (sim.strips["A"].param("fader", "gain").unwrap_or(1.0) - 0.25).abs() < 1e-6,
            "sim stopped at the unknown params, gain is {}",
            sim.strips["A"].param("fader", "gain").unwrap_or(1.0)
        );
    }

    fn check_sim_vs_engine(events: &[SessionEvent], cache: &SampleCache, seconds: usize) {
        check_sim_vs_engine_deck(events, cache, seconds, "A");
    }

    fn check_sim_vs_engine_deck(
        events: &[SessionEvent],
        cache: &SampleCache,
        seconds: usize,
        deck: &str,
    ) {
        let (max_diff, worst_t) = max_sim_engine_divergence(events, cache, seconds, deck);
        assert!(
            max_diff < 2.0,
            "sim_pos diverges from real engine by {max_diff} frames at t={worst_t}ms"
        );
    }

    const PARITY_BLOCK: usize = 512;

    fn max_sim_engine_divergence(
        events: &[SessionEvent],
        cache: &SampleCache,
        seconds: usize,
        deck: &str,
    ) -> (f64, f64) {
        let mut sorted: Vec<&SessionEvent> = events
            .iter()
            .filter(|e| e.deck.as_deref() == Some(deck))
            .collect();
        sorted.sort_by(|a, b| a.elapsed_ms.partial_cmp(&b.elapsed_ms).unwrap());

        let mut d = Deck::empty(SR);
        let mut s = ChannelStrip::new(SR_F as f32);
        let total_frames = seconds * SR as usize;
        let mut ei = 0usize;
        let mut max_diff = 0.0f64;
        let mut worst_t = 0.0f64;
        for n in 0..total_frames {
            let cur_ms = n as f64 / SR_F * 1000.0;
            while ei < sorted.len() && sorted[ei].elapsed_ms <= cur_ms {
                apply_deck_event(&mut d, &mut s, sorted[ei], cache);
                ei += 1;
            }
            if n % (SR as usize / 10) == 0 && n > 0 {
                if let Some(sim) = playthrough_pos(events, cur_ms, cache, deck) {
                    let mut actual = d.main_pos;
                    if d.loop_active && d.loop_end > d.cue_point && actual >= d.loop_end {
                        let dur = d.loop_end - d.cue_point;
                        actual = d.cue_point + (actual - d.loop_end) % dur;
                    }
                    let diff = (sim - actual).abs();
                    if diff > max_diff {
                        max_diff = diff;
                        worst_t = cur_ms;
                    }
                }
            }
            // The wheel is drained once per block by `render_block`, so a per-frame model
            // that skips it leaves every logged tick unconsumed.
            if n % PARITY_BLOCK == 0 {
                d.consume_jog(PARITY_BLOCK);
            }
            d.main_tick();
        }
        (max_diff, worst_t)
    }

    #[test]
    fn sim_pos_matches_real_engine_for_realistic_session() {
        let path = "/fake/a.wav".to_string();
        let mut cache: SampleCache = HashMap::new();
        cache.insert(
            path.clone(),
            (Arc::new(vec![0.0f32; 60 * SR as usize * 2]), 2),
        );

        let mk = |t: f64, ty: &str| SessionEvent {
            event_type: ty.to_string(),
            elapsed_ms: t,
            deck: Some("A".to_string()),
            ..Default::default()
        };

        let events = vec![
            SessionEvent {
                path: Some(path.clone()),
                is_playing: Some(true),
                playback_rate: Some(1.0),
                position_sec: Some(0.0),
                ..mk(0.0, "deck_snapshot")
            },
            SessionEvent {
                rate: Some(1.03),
                ..mk(4000.0, "set_playback_rate")
            },
            SessionEvent {
                start_sec: Some(8.0),
                end_sec: Some(10.0),
                ..mk(10_000.0, "loop_out")
            },
            mk(18_000.0, "exit_loop"),
        ];

        // Drive the real engine once to 25s, checking sim_pos every 100ms.
        check_sim_vs_engine(&events, &cache, 25);
    }

    #[test]
    fn sim_pos_matches_real_engine_non_loop_beatmatch() {
        let path = "/fake/a.wav".to_string();
        let mut cache: SampleCache = HashMap::new();
        cache.insert(
            path.clone(),
            (Arc::new(vec![0.0f32; 60 * SR as usize * 2]), 2),
        );

        let mk = |t: f64, ty: &str| SessionEvent {
            event_type: ty.to_string(),
            elapsed_ms: t,
            deck: Some("A".to_string()),
            ..Default::default()
        };

        // No loops: snapshot playing, rate trims, nudges (held + released),
        // a cue preview (press/release), a stop/play, and a seek.
        let events = vec![
            SessionEvent {
                path: Some(path.clone()),
                is_playing: Some(true),
                playback_rate: Some(1.0),
                position_sec: Some(0.0),
                cue_point_sec: Some(0.0),
                ..mk(0.0, "deck_snapshot")
            },
            SessionEvent {
                rate: Some(1.021),
                ..mk(3000.0, "set_playback_rate")
            },
            SessionEvent {
                percent: Some(3.0),
                ..mk(5000.0, "set_nudge")
            },
            SessionEvent {
                percent: Some(0.0),
                ..mk(5350.0, "set_nudge")
            },
            SessionEvent {
                percent: Some(-2.5),
                ..mk(8000.0, "set_nudge")
            },
            SessionEvent {
                percent: Some(0.0),
                ..mk(8200.0, "set_nudge")
            },
            SessionEvent {
                cue_point_sec: Some(4.0),
                ..mk(12_000.0, "cue_preview_start")
            },
            SessionEvent {
                cue_point_sec: Some(4.0),
                ..mk(12_800.0, "cue_preview_end")
            },
            mk(14_000.0, "play"),
            mk(16_000.0, "stop"),
            mk(18_000.0, "play"),
            SessionEvent {
                sec: Some(25.0),
                ..mk(20_000.0, "seek")
            },
        ];

        check_sim_vs_engine(&events, &cache, 24);
    }

    #[test]
    fn sim_pos_matches_real_engine_for_loop_in_reloop_cycle() {
        let path = "/fake/a.wav";
        let cache = cached_track(path, 60);
        let events = loop_cycle_events(path);
        check_sim_vs_engine(&events, &cache, 20);
    }

    // Compile-time guard: a new SessionCommand variant fails here until it is
    // classified. Playhead movers must then appear in full_coverage_events.
    fn variant_catalog(cmd: &SessionCommand) -> (&'static str, bool) {
        match cmd {
            SessionCommand::DeckSnapshot { .. } => ("deck_snapshot", true),
            SessionCommand::LoadTrack { .. } => ("load_track", true),
            SessionCommand::EjectTrack { .. } => ("eject_track", true),
            SessionCommand::Play { .. } => ("play", true),
            SessionCommand::Stop { .. } => ("stop", true),
            SessionCommand::StopAtCue { .. } => ("stopped_at_cue", true),
            SessionCommand::Seek { .. } => ("seek", true),
            SessionCommand::SetPlaybackRate { .. } => ("set_playback_rate", true),
            SessionCommand::SetNudge { .. } => ("set_nudge", true),
            SessionCommand::LoopIn { .. } => ("loop_in", true),
            SessionCommand::LoopOut { .. } => ("loop_out", true),
            SessionCommand::ExitLoop { .. } => ("exit_loop", true),
            SessionCommand::Reloop { .. } => ("reloop", true),
            SessionCommand::CuePreviewStart { .. } => ("cue_preview_start", true),
            SessionCommand::CuePreviewEnd { .. } => ("cue_preview_end", true),
            SessionCommand::SetBeatGrid { .. } => ("set_beat_grid", false),
            // No mixer param affects position, hence the single false.
            SessionCommand::SetParam { .. } => ("set_param", false),
            SessionCommand::SetXfaderAssign { .. } => ("set_xfader_assign", false),
            SessionCommand::SetFaderCurve { .. } => ("set_fader_curve", false),
            SessionCommand::SetJogRotationSpeed { .. } => ("set_jog_rotation_speed", false),
            SessionCommand::Jog { .. } => ("jog", true),
        }
    }

    // The `true` arms of variant_catalog. Coverage_list_matches_catalog binds them.
    const POSITION_AFFECTING_TAGS: [&str; 16] = [
        "jog",
        "deck_snapshot",
        "load_track",
        "eject_track",
        "play",
        "stop",
        "stopped_at_cue",
        "seek",
        "set_playback_rate",
        "set_nudge",
        "loop_in",
        "loop_out",
        "exit_loop",
        "reloop",
        "cue_preview_start",
        "cue_preview_end",
    ];

    // One coherent ~30s timeline on deck A that exercises every command variant.
    // Position-affecting events are checked against the real engine every 100ms;
    // the strip/master/beat-grid events are along for the ride (they must not
    // perturb the playhead).
    fn full_coverage_events(path: &str) -> Vec<SessionEvent> {
        vec![
            SessionEvent {
                path: Some(path.to_string()),
                is_playing: Some(true),
                playback_rate: Some(1.0),
                position_sec: Some(0.0),
                cue_point_sec: Some(0.0),
                ..deck_ev("deck_snapshot", 0.0, "A")
            },
            SessionEvent {
                bpm: Some(128.0),
                beat_offset_sec: Some(0.0),
                ..deck_ev("set_beat_grid", 1000.0, "A")
            },
            SessionEvent {
                rate: Some(1.03),
                ..deck_ev("set_playback_rate", 2000.0, "A")
            },
            SessionEvent {
                percent: Some(2.0),
                ..deck_ev("set_nudge", 3000.0, "A")
            },
            SessionEvent {
                percent: Some(0.0),
                ..deck_ev("set_nudge", 3500.0, "A")
            },
            SessionEvent {
                ticks: Some(6.0),
                ..deck_ev("jog", 3700.0, "A")
            },
            SessionEvent::param(4000.0, Some("A"), "fader", "gain", 0.8),
            SessionEvent::param(4100.0, Some("A"), "eq", "low", -3.0),
            SessionEvent::param(4200.0, Some("A"), "filter", "value", 0.5),
            SessionEvent::param(4300.0, Some("A"), "filter", "active", 1.0),
            SessionEvent::param(4500.0, None, "gain", "gain", 0.9),
            SessionEvent {
                cue_sec: Some(5.0),
                ..deck_ev("loop_in", 5000.0, "A")
            },
            SessionEvent {
                start_sec: Some(5.0),
                end_sec: Some(7.0),
                ..deck_ev("loop_out", 6000.0, "A")
            },
            deck_ev("exit_loop", 11_000.0, "A"),
            deck_ev("reloop", 13_000.0, "A"),
            SessionEvent {
                sec: Some(20.0),
                ..deck_ev("seek", 16_000.0, "A")
            },
            deck_ev("stop", 17_000.0, "A"),
            SessionEvent {
                cue_point_sec: Some(20.0),
                ..deck_ev("cue_preview_start", 18_000.0, "A")
            },
            SessionEvent {
                cue_point_sec: Some(20.0),
                ..deck_ev("cue_preview_end", 18_800.0, "A")
            },
            deck_ev("play", 19_000.0, "A"),
            SessionEvent {
                cue_point_sec: Some(20.0),
                ..deck_ev("stopped_at_cue", 21_000.0, "A")
            },
            SessionEvent {
                sec: Some(20.0),
                ..deck_ev("play", 22_000.0, "A")
            },
            deck_ev("eject_track", 24_000.0, "A"),
            SessionEvent {
                path: Some(path.to_string()),
                beat_offset_sec: Some(0.0),
                ..deck_ev("load_track", 25_000.0, "A")
            },
            deck_ev("play", 25_500.0, "A"),
        ]
    }

    #[test]
    fn coverage_list_matches_catalog() {
        // Every tag in the list is a position-affecting variant per the catalog,
        // and the list has no duplicates: binds the hand-maintained list to the
        // exhaustive match so the two cannot silently drift apart.
        let unique: HashSet<&str> = POSITION_AFFECTING_TAGS.iter().copied().collect();
        assert_eq!(
            unique.len(),
            POSITION_AFFECTING_TAGS.len(),
            "POSITION_AFFECTING_TAGS has duplicates"
        );
    }

    #[test]
    fn full_coverage_events_exercise_every_position_variant() {
        let events = full_coverage_events("/fake/a.wav");
        let mut seen: HashSet<&str> = HashSet::new();
        for event in &events {
            let cmd = event.command().unwrap_or_else(|| {
                panic!("event {} did not convert to a command", event.event_type)
            });
            let (tag, position_affecting) = variant_catalog(&cmd);
            assert_eq!(tag, event.event_type, "catalog tag mismatch for {tag}");
            if position_affecting {
                seen.insert(tag);
            }
        }
        for tag in POSITION_AFFECTING_TAGS {
            assert!(
                seen.contains(tag),
                "full_coverage_events does not exercise position-affecting variant `{tag}`"
            );
        }
    }

    #[test]
    fn sim_matches_engine_for_every_variant() {
        let path = "/fake/a.wav";
        let cache = cached_track(path, 60);
        let events = full_coverage_events(path);
        check_sim_vs_engine(&events, &cache, 30);
    }

    #[test]
    fn sim_matches_engine_when_loop_engages_below_loop_start() {
        let path = "/fake/a.wav";
        let cache = cached_track(path, 60);
        let events = vec![
            playing_snapshot(path),
            SessionEvent {
                start_sec: Some(8.0),
                end_sec: Some(10.0),
                ..deck_ev("loop_out", 1000.0, "A")
            },
            deck_ev("exit_loop", 14_000.0, "A"),
        ];
        check_sim_vs_engine(&events, &cache, 16);
    }
}
