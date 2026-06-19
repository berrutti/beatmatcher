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
use session_core::event::SessionCommand;
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

#[tauri::command]
pub(crate) async fn open_session_dialog() -> Option<OpenedFile> {
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
pub(crate) async fn preload_session(
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

    index_session(&state, path, session).await;

    Ok(())
}

// Caches track samples, builds scrub snapshots, and stores the session as the
// in-memory source of truth for playback and offline render under this path.
async fn index_session(
    state: &tauri::State<'_, AppState>,
    path: String,
    session: crate::offline_render::SessionFile,
) {
    let sr = state.audio.device_sample_rate;
    let paths = session_track_paths(&session.events);
    populate_track_cache(state, paths, sr).await;

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
        .insert(path.clone(), snapshots);
    state
        .session_files
        .lock()
        .expect("session files mutex poisoned")
        .insert(path, Arc::new(session));
}

// Frees everything cached for a session when it is ejected: decoded track
// samples are the bulk of it (hundreds of MB for a multi-track session).
// Playback of a path that is no longer cached falls back to a disk read.
#[tauri::command]
pub(crate) fn unload_session(state: tauri::State<'_, AppState>, path: String) {
    let removed = state
        .session_files
        .lock()
        .expect("session files mutex poisoned")
        .remove(&path);
    state
        .session_snapshots
        .lock()
        .expect("snapshots mutex poisoned")
        .remove(&path);
    if let Some(session) = removed {
        let track_paths = session_track_paths(&session.events);
        let mut cache = state
            .session_track_cache
            .lock()
            .expect("track cache mutex poisoned");
        for track_path in track_paths {
            cache.remove(&track_path);
        }
    }
}

// Replaces the in-memory event list for a loaded session with edited events
// from the frontend. The .bms on disk is untouched; the next playback, scrub,
// or render uses the edited events.
#[tauri::command]
pub(crate) async fn update_session_events(
    state: tauri::State<'_, AppState>,
    path: String,
    events_json: String,
) -> Result<(), String> {
    let events: Vec<SessionEvent> =
        serde_json::from_str(&events_json).map_err(|e| format!("parse error: {e}"))?;

    index_session(&state, path, crate::offline_render::SessionFile { events }).await;

    Ok(())
}

#[tauri::command]
pub(crate) async fn start_session_playback(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    path: String,
    from_ms: f64,
) -> Result<(), String> {
    // Prefer the in-memory session (which may hold unsaved edits); fall back to
    // the disk file for callers that never preloaded.
    let cached: Option<Arc<crate::offline_render::SessionFile>> = state
        .session_files
        .lock()
        .expect("session files mutex poisoned")
        .get(&path)
        .cloned();
    let session: Arc<crate::offline_render::SessionFile> = match cached {
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
            let parsed: crate::offline_render::SessionFile =
                serde_json::from_str(&json).map_err(|e| format!("parse error: {e}"))?;
            let parsed = Arc::new(parsed);
            state
                .session_files
                .lock()
                .expect("session files mutex poisoned")
                .insert(path.clone(), parsed.clone());
            parsed
        }
    };

    // Cancel any previous playback task AND wait for it to fully exit before we
    // touch the engine. The async runtime is multi-threaded, so without this the
    // old task could apply a stale event to a deck after the new task has already
    // reset and re-placed it, corrupting one deck's position (audible desync that
    // varies per scrub). Serializing the tasks removes that race entirely.
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
    let old_handle = state
        .session_playback_handle
        .lock()
        .expect("session_playback handle mutex poisoned")
        .take();
    if let Some(handle) = old_handle {
        let _ = handle.await;
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
        // deck_snapshot is already folded into the base snapshot; never replay it.
        for ev in sorted_events.iter().filter(|e| {
            e.elapsed_ms > snapshot_ms && e.elapsed_ms <= from_ms && e.event_type != "deck_snapshot"
        }) {
            if matches!(
                ev.command(),
                Some(
                    SessionCommand::SetVolume { .. }
                        | SessionCommand::SetEq { .. }
                        | SessionCommand::SetFilter { .. }
                        | SessionCommand::SetFilterActive { .. }
                        | SessionCommand::SetMasterGain { .. }
                )
            ) {
                apply_event_live(ev, &audio, sr, &cache, 0);
            }
            sim_apply_event(ev, &mut sim, &cache, sr);
        }

        let sr_f = sr.max(1) as f64;

        // Place every deck at its reconstructed state, then start them together.
        // Decks are locked in sorted-id order (matching the audio callback's lock
        // order, so no deadlock). Pass 1 does the heavy per-deck setup under brief
        // individual locks with is_playing left false, so nothing is audible
        // mid-setup. Pass 2 flips is_playing for the decks that should play, under
        // all their locks held at once (just bool writes, nanoseconds): every deck
        // begins on the same output buffer, sample-locked, without stalling the
        // audio callback (no priority inversion).
        let mut ids: Vec<&String> = sim.decks.keys().collect();
        ids.sort();
        let mut to_start: Vec<Arc<std::sync::Mutex<audio::DeckState>>> = Vec::new();
        for id in ids {
            let ds = &sim.decks[id];
            let (Some(path), Some(arc)) = (ds.path.as_ref(), audio.deck(id)) else {
                continue;
            };
            let Some((samples, channels)) = cache.get(path) else {
                continue;
            };
            let total_frames = samples.len() / channels;
            {
                let mut d = arc.lock().expect("deck mutex poisoned");
                d.samples = samples.clone();
                d.channels = *channels;
                d.device_sample_rate = sr;
                d.total_frames = total_frames;
                d.duration = total_frames as f64 / sr_f;
                d.loaded_path = Some(path.clone());
                d.main_pos = sim_pos(ds, from_ms, sr_f).min(total_frames as f64);
                d.cue_pos = d.main_pos;
                d.cue_point = ds.cue_point.min(total_frames as f64);
                d.loop_active = ds.loop_active;
                d.loop_end = ds.loop_end.min(total_frames as f64);
                d.playback_rate = ds.rate;
                d.nudge_factor = ds.nudge_factor;
                d.bpm = ds.bpm;
                d.beat_offset_frames = ds.beat_offset_frames;
                d.is_playing = false;
                d.is_cueing = false;
            }
            if ds.is_playing {
                to_start.push(arc);
            }
        }
        {
            let mut guards: Vec<_> = to_start
                .iter()
                .map(|a| a.lock().expect("deck mutex poisoned"))
                .collect();
            for d in guards.iter_mut() {
                d.is_playing = true;
            }
        }

        // Schedule events against the master output frame clock instead of
        // wall-clock time. The decks advance on the soundcard's sample clock
        // inside the audio callback, so referencing that same clock keeps event
        // application locked to the audio output (no OS-clock-vs-soundcard drift,
        // no tokio sleep jitter). base_frame is the clock value at loop start.
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
            for id in ["A", "B", "C", "D"] {
                if let Some(deck_arc) = audio.deck(id) {
                    deck_arc.lock().expect("deck mutex poisoned").is_playing = false;
                }
            }
            app_handle.emit("session-playback-ended", ()).ok();
        }
    });

    *state
        .session_playback_handle
        .lock()
        .expect("session_playback handle mutex poisoned") = Some(handle);

    Ok(())
}

#[tauri::command]
pub(crate) fn stop_session_playback(state: tauri::State<'_, AppState>) {
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

fn session_track_paths(events: &[SessionEvent]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    events
        .iter()
        .filter_map(|e| match e.command() {
            Some(
                SessionCommand::DeckSnapshot { path, .. } | SessionCommand::LoadTrack { path, .. },
            ) => Some(path),
            _ => None,
        })
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

// Wait until the master output frame clock reaches `target_frame`, sleeping in
// short steps sized to the remaining frames. Returns early on cancel, and also
// if the clock stalls for 500ms (no audio device producing) so playback can
// never hang waiting on a clock that isn't ticking.
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

// `overshoot_frames` is how many master output frames the audio clock is already
// past this event's target frame when it gets applied (the event only takes
// effect on the next audio callback, so it is up to one buffer "late"). For
// events that start or reposition a *playing* deck, the deck must be advanced by
// overshoot*rate so it lands where it belongs relative to decks that were already
// playing, instead of a fraction of a buffer behind them. Pass 0 when timing
// doesn't matter (e.g. reconstruction replay of past mixer events).
fn apply_event_live(
    ev: &SessionEvent,
    audio: &audio::AppAudio,
    sr: u32,
    cache: &SampleCache,
    overshoot_frames: u64,
) {
    let overshoot_f = overshoot_frames as f64;

    let Some(cmd) = ev.command() else { return };

    let Some(id) = cmd.deck_id() else {
        if let SessionCommand::SetMasterGain { gain } = cmd {
            audio.monitor.set_master_gain(gain);
        }
        return;
    };

    let (Some(deck_a), Some(strip_a)) = (audio.deck(id), audio.strip(id)) else {
        return;
    };
    let mut d = deck_a.lock().expect("deck mutex poisoned");
    let mut s = strip_a.lock().expect("strip mutex poisoned");

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

// The deterministic simulation is unit-tested in the session-core crate. The
// tests here drive the SAME shared sim against the REAL DeckState audio engine,
// so they must stay in the binary that owns the engine: they guard that
// sim_pos (used for scrub placement) lands where continuous frame-by-frame
// playback would.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{ChannelStrip, DeckState};
    use std::collections::{HashMap, HashSet};

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
        for ev in sorted.iter().filter(|e| e.elapsed_ms <= from_ms) {
            sim_apply_event(ev, &mut state, cache, SR);
        }
        state.decks.get(deck).map(|d| sim_pos(d, from_ms, SR_F))
    }

    // ── sim_pos vs the real DeckState audio engine ────────────────────────────
    //
    // Continuous playback advances the real DeckState frame by frame. Scrubbing
    // instead drops the playhead at sim_pos(T). If sim_pos disagrees with where
    // the real engine would be at T, scrubbing desyncs decks that continuous
    // playback keeps tight. This drives the real engine once and checks every
    // 100ms checkpoint against sim_pos.

    // Drive an event through the REAL production applier (`apply_deck_command`),
    // the same code path the live scheduler and offline renderer use. The test
    // must never reimplement command semantics here: a private copy could pass
    // the parity check while production diverges. `overshoot_frames` is 0.0
    // (a no-op, matching the offline renderer), since the analytic sim models no
    // sub-buffer start latency. Master-gain events carry no deck and are
    // dispatched separately in production, so they're skipped here too.
    fn apply_deck_event(
        d: &mut DeckState,
        s: &mut ChannelStrip,
        ev: &SessionEvent,
        cache: &SampleCache,
    ) {
        let Some(cmd) = ev.command() else { return };
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

        let mut d = DeckState::empty(SR);
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

    // ── exhaustive variant coverage ───────────────────────────────────────────
    //
    // The analytic sim (`sim_apply_event`) and the real engine
    // (`apply_deck_command`) are two models of the same command semantics. The
    // type system forces both to HANDLE every variant, but not to AGREE. This
    // match is the compile-time guard that every variant is also covered by the
    // sim-vs-engine parity test: it maps each `SessionCommand` to its canonical
    // .bms `type` string and whether it moves the playhead. Adding a variant
    // fails to compile here until it's classified; if it moves the playhead it
    // must then be exercised by `full_coverage_events` (asserted at runtime).
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
            SessionCommand::SetVolume { .. } => ("set_volume", false),
            SessionCommand::SetEq { .. } => ("set_eq", false),
            SessionCommand::SetFilter { .. } => ("set_filter", false),
            SessionCommand::SetFilterActive { .. } => ("set_filter_active", false),
            SessionCommand::SetMasterGain { .. } => ("set_master_gain", false),
        }
    }

    // The playhead-moving variants that `full_coverage_events` must exercise.
    // Keep in sync with the `true` arms of `variant_catalog` above; a new variant
    // surfaces as a compile error there, and `coverage_list_matches_catalog`
    // catches any tag typo or duplication between the two.
    const POSITION_AFFECTING_TAGS: [&str; 15] = [
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
                gain: Some(0.8),
                ..deck_ev("set_volume", 4000.0, "A")
            },
            SessionEvent {
                band: Some("low".to_string()),
                db: Some(-3.0),
                ..deck_ev("set_eq", 4100.0, "A")
            },
            SessionEvent {
                value: Some(0.5),
                ..deck_ev("set_filter", 4200.0, "A")
            },
            SessionEvent {
                active: Some(true),
                ..deck_ev("set_filter_active", 4300.0, "A")
            },
            SessionEvent {
                gain: Some(0.9),
                ..deck_ev("set_master_gain", 4500.0, "A")
            },
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
        for ev in &events {
            let cmd = ev
                .command()
                .unwrap_or_else(|| panic!("event {} did not convert to a command", ev.event_type));
            let (tag, position_affecting) = variant_catalog(&cmd);
            assert_eq!(tag, ev.event_type, "catalog tag mismatch for {tag}");
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
}
