use crate::audio::{self, DeviceInfo, TrackInfo};
use crate::deck_sync::DeckSyncPayload;
use crate::engine::{LoopOutResult, NudgeResult};
use crate::lock::LockIgnoringPoison;
use crate::ParamOrigin;
use std::sync::Arc;
use tauri::{Emitter, Manager};

/// Small enough that the interval below decides the update rate, not the chunk size.
const POINTS_PER_CHUNK: usize = 64;
/// One update a frame, so the waveform fills in smoothly instead of in visible blocks.
const PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(16);

/// Emitted while the track is being analysed, so a drawer can paint what has arrived.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct WaveformProgress {
    deck: String,
    points_ready: usize,
    total_points: usize,
    points_per_sec: f64,
}

/// What `bands-ready` carries, so the frontend can read a band against its own average
/// instead of the track's without a second round trip.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BandsReady {
    deck: String,
}

/// Where the crossfader was when recording started. Thru is skipped because every reader
/// already defaults to it, and stamping it would head every session with inert events.
fn crossfader_start_events(
    position: f32,
    assigns: &[(&str, session_core::XfaderAssign)],
) -> Vec<(&'static str, serde_json::Value)> {
    let mut events = vec![(
        "set_param",
        serde_json::json!({ "slot": "xfader", "param": "position", "value": crate::recorder::f32_json(position) }),
    )];
    for (deck, assign) in assigns {
        if *assign == session_core::XfaderAssign::Thru {
            continue;
        }
        events.push((
            "set_xfader_assign",
            serde_json::json!({ "deck": deck, "assign": assign.as_str() }),
        ));
    }
    events
}

/// Where a deck's strip stood when recording started. A knob moved before the first event
/// is otherwise lost, and a reader replays the manifest default in its place. Params
/// already at their default are skipped, as `crossfader_start_events` skips thru.
fn strip_start_events(
    manifest: &'static session_core::MixerManifest,
    deck_id: &'static str,
    read: impl Fn(&str, &str) -> Option<f32>,
) -> Vec<(&'static str, serde_json::Value)> {
    let mut events = Vec::new();
    for slot in manifest.strip {
        for param in slot.params {
            let Some(value) = read(slot.slot, param.id) else {
                continue;
            };
            if f64::from(value) == param.default {
                continue;
            }
            events.push((
                "set_param",
                serde_json::json!({
                    "deck": deck_id,
                    "slot": slot.slot,
                    "param": param.id,
                    "value": crate::recorder::f32_json(value),
                }),
            ));
        }
    }
    events
}

/// A tick is worth 60/rpm seconds of audio, so a session that omits this cannot say how
/// far its scrubs travelled. Stamped for the same reason the fader curve is.
fn jog_speed_start_event(
    speed: session_core::JogRotationSpeed,
) -> (&'static str, serde_json::Value) {
    (
        "set_jog_rotation_speed",
        serde_json::json!({ "speed": speed.as_str() }),
    )
}

fn xfader_assigns(
    audio: &crate::audio::AppAudio,
) -> Vec<(&'static str, session_core::XfaderAssign)> {
    crate::audio::LIVE_DECK_IDS
        .into_iter()
        .map(|deck_id| {
            let assign = audio
                .strip(deck_id)
                .map(|strip| strip.locked().xfader_assign)
                .unwrap_or_default();
            (deck_id, assign)
        })
        .collect()
}

/// None for a deck with nothing loaded, which has no state worth restoring.
fn deck_snapshot_event(
    audio: &crate::audio::AppAudio,
    deck_id: &'static str,
) -> Option<(&'static str, serde_json::Value)> {
    let arc = audio.deck(deck_id)?;
    // Read the strip gain before locking the deck so the cue sheet
    // knows whether a deck already playing at record start is audible.
    let gain = audio
        .strip(deck_id)
        .map(|strip| strip.locked().target_gain())
        .unwrap_or(1.0);
    let deck = arc.locked();
    let path = deck.loaded_path.as_ref()?;
    let sample_rate = deck.device_sample_rate as f64;
    Some((
        "deck_snapshot",
        serde_json::json!({
            "deck": deck_id,
            "path": path,
            "position_sec": deck.main_pos / sample_rate,
            "cue_point_sec": deck.cue_point / sample_rate,
            "is_playing": deck.is_playing,
            "gain": gain,
            "bpm": deck.bpm,
            "playback_rate": deck.playback_rate,
            "loop_active": deck.loop_active,
            "loop_end_sec": deck.loop_end / sample_rate,
        }),
    ))
}

/// Everything a session has to say about the state it started from, in the order it
/// is written. A reader applies all of it before the first performed move.
fn start_events(audio: &crate::audio::AppAudio) -> Vec<(&'static str, serde_json::Value)> {
    let mut events = vec![
        (
            "recording_start",
            serde_json::json!({
                "buffer_size_frames": audio.monitor.frames_per_callback(),
                "sample_rate": audio.device_sample_rate,
                "limiter_enabled": audio.monitor.limiter_enabled(),
            }),
        ),
        // A setting rather than a performed move, so nothing else in the
        // session would ever say which curve the fader moves were played on.
        (
            "set_fader_curve",
            serde_json::json!({ "curve": audio.monitor.fader_curve().as_str() }),
        ),
        jog_speed_start_event(audio.jog_rotation_speed()),
    ];
    events.extend(crossfader_start_events(
        audio.monitor.xfader_position(),
        &xfader_assigns(audio),
    ));
    for deck_id in crate::audio::LIVE_DECK_IDS {
        let Some(arc) = audio.strip(deck_id) else {
            continue;
        };
        let strip = arc.locked();
        events.extend(strip_start_events(audio.mixer(), deck_id, |slot, param| {
            strip.param(slot, param)
        }));
    }
    events.extend(
        crate::audio::LIVE_DECK_IDS
            .into_iter()
            .filter_map(|deck_id| deck_snapshot_event(audio, deck_id)),
    );
    events
}

// Mirrors the "filename (1).ext" pattern browsers use for repeat downloads,
// since this path is auto-derived from the audio filename and never goes
// through a save dialog the user could redirect away from an existing file.
fn strip_audio_extension(path: &str) -> &str {
    path.strip_suffix(".wav")
        .or_else(|| path.strip_suffix(".WAV"))
        .or_else(|| path.strip_suffix(".flac"))
        .or_else(|| path.strip_suffix(".FLAC"))
        .unwrap_or(path)
}

fn unique_path(path: &str) -> String {
    if !std::path::Path::new(path).exists() {
        return path.to_string();
    }
    let path = std::path::Path::new(path);
    let parent = path.parent();
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let extension = path.extension().and_then(|s| s.to_str());
    let mut counter = 1;
    loop {
        let candidate_name = match extension {
            Some(extension) => format!("{stem} ({counter}).{extension}"),
            None => format!("{stem} ({counter})"),
        };
        let candidate = match parent {
            Some(parent) if !parent.as_os_str().is_empty() => parent.join(&candidate_name),
            _ => std::path::PathBuf::from(&candidate_name),
        };
        if !candidate.exists() {
            return candidate.to_string_lossy().into_owned();
        }
        counter += 1;
    }
}

const CUE_SET_TITLE_FALLBACK: &str = "Mix";
const CUE_TRACK_TITLE_FALLBACK: &str = "Track";

fn cue_escape(value: &str) -> String {
    value.replace('"', "'")
}

// CUE timestamps are MM:SS:FF where FF is frames at 75 per second (CD-DA).
fn cue_timecode(elapsed_ms: f64) -> String {
    let total_frames = (elapsed_ms.max(0.0) / 1000.0 * 75.0).round() as u64;
    let minutes = total_frames / (75 * 60);
    let seconds = (total_frames / 75) % 60;
    let frames = total_frames % 75;
    format!("{minutes:02}:{seconds:02}:{frames:02}")
}

// Tag lookup is injected so a test can pin the formatting without the filesystem.
fn cue_sheet_text(
    file_name: &str,
    set_title: &str,
    points: &[session_core::CuePoint],
    tags_for: impl Fn(&str) -> audio::TrackTags,
) -> String {
    let mut sheet = String::new();
    sheet.push_str(&format!("TITLE \"{}\"\n", cue_escape(set_title)));
    sheet.push_str(&format!("FILE \"{}\" WAVE\n", cue_escape(file_name)));
    for (index, point) in points.iter().enumerate() {
        let tags = tags_for(&point.track_path);
        let fallback = std::path::Path::new(&point.track_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(CUE_TRACK_TITLE_FALLBACK)
            .to_string();
        sheet.push_str(&format!("  TRACK {:02} AUDIO\n", index + 1));
        sheet.push_str(&format!(
            "    TITLE \"{}\"\n",
            cue_escape(&tags.title.unwrap_or(fallback))
        ));
        if let Some(artist) = tags.artist {
            sheet.push_str(&format!("    PERFORMER \"{}\"\n", cue_escape(&artist)));
        }
        sheet.push_str(&format!(
            "    INDEX 01 {}\n",
            cue_timecode(point.elapsed_ms)
        ));
    }
    sheet
}

// FILE references the audio by name only, so the .cue must sit beside it.
fn write_cue_sheet(audio_path: &str, events: &[session_core::SessionEvent]) {
    let points = session_core::build_cue_points(events);
    if points.is_empty() {
        log::warn!(
            "cue sheet not written: no audible tracks found in the recording ({} events)",
            events.len()
        );
        return;
    }
    let audio_file = std::path::Path::new(audio_path);
    // Without a filename there is nothing valid to put in the FILE line, so skip
    // writing a broken sheet entirely.
    let Some(file_name) = audio_file.file_name().and_then(|s| s.to_str()) else {
        log::warn!("cue sheet not written: no filename in {audio_path}");
        return;
    };
    let set_title = audio_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(CUE_SET_TITLE_FALLBACK);

    let sheet = cue_sheet_text(file_name, set_title, &points, audio::read_tags);

    let cue_path = unique_path(&audio_file.with_extension("cue").to_string_lossy());
    match std::fs::write(&cue_path, sheet.as_bytes()) {
        Ok(()) => log::info!("wrote cue sheet {cue_path} ({} tracks)", points.len()),
        Err(error) => log::warn!("cue sheet write failed for {cue_path}: {error}"),
    }
}

fn scan_dir_recursive(dir: &std::path::Path, results: &mut Vec<String>) {
    const AUDIO_EXT: &[&str] = &["mp3", "wav", "flac", "aac", "ogg", "m4a", "aif", "aiff"];
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<_> = entries.flatten().collect();
    paths.sort_by_key(|entry| entry.file_name());
    for entry in paths {
        let path = entry.path();
        if path.is_dir() {
            scan_dir_recursive(&path, results);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if AUDIO_EXT.iter().any(|&e| e.eq_ignore_ascii_case(ext)) {
                if let Some(p) = path.to_str() {
                    results.push(p.to_string());
                }
            }
        }
    }
}

/// Mode is owned by the frontend, and this is the mirror the MIDI thread reads. Clearing
/// the memory matches reconnect: a half stranded by a mode switch would join across it.
#[tauri::command]
pub fn set_app_mode(
    engine: tauri::State<'_, crate::engine::Engine>,
    surface: tauri::State<'_, crate::SurfaceControl>,
    midi: tauri::State<'_, crate::midi::MidiState>,
    mode: crate::AppMode,
) {
    surface.allow(mode);
    midi.clear_control_memory();
    // The gate in `midi::apply` drops the release edge of anything still held, so the hold
    // is ended here rather than left latched with the button already back up.
    engine.audio.release_held_controls();
}

#[tauri::command]
pub(crate) fn stop(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
) -> Result<(), String> {
    engine.stop(&deck)
}

#[tauri::command]
pub(crate) fn press_cue(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
) -> Result<DeckSyncPayload, String> {
    engine.press_cue(ParamOrigin::Ui, &deck)
}

#[tauri::command]
pub(crate) fn release_cue(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
) -> Result<DeckSyncPayload, String> {
    engine.release_cue(ParamOrigin::Ui, &deck)
}

#[tauri::command]
pub(crate) fn toggle_play(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
) -> Result<DeckSyncPayload, String> {
    engine.toggle_play(ParamOrigin::Ui, &deck)
}

#[tauri::command]
pub(crate) fn seek(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
    sec: f64,
) -> Result<DeckSyncPayload, String> {
    engine.seek(&deck, sec)
}

#[tauri::command]
pub(crate) fn set_pitch_offset(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
    percent: f64,
) -> Result<f64, String> {
    engine.set_playback_rate_from_offset(ParamOrigin::Ui, &deck, percent)
}

#[tauri::command]
pub(crate) fn set_beat_grid(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
    bpm: Option<f64>,
    beat_offset_sec: f64,
) -> Result<(), String> {
    engine.set_beat_grid(&deck, bpm, beat_offset_sec)
}

#[tauri::command]
pub(crate) fn set_playback_rate(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
    rate: f64,
) -> Result<(), String> {
    engine.set_playback_rate(ParamOrigin::Ui, &deck, rate)
}

#[tauri::command]
pub(crate) fn set_jog_rotation_speed(
    engine: tauri::State<'_, crate::engine::Engine>,
    speed: session_core::JogRotationSpeed,
) {
    engine.audio.set_jog_rotation_speed(speed);
}

#[tauri::command]
pub(crate) fn set_nudge(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
    percent: f64,
) -> Result<NudgeResult, String> {
    engine.set_nudge(&deck, percent)
}

#[tauri::command]
pub(crate) fn set_quantize(
    engine: tauri::State<'_, crate::engine::Engine>,
    midi: tauri::State<'_, crate::midi::MidiState>,
    deck: String,
    quantize: bool,
) -> Result<(), String> {
    engine.deck(&deck)?.locked().set_quantize(quantize);
    crate::midi::refresh_led(&engine, &midi, crate::midi::Feedback::Quantize, &deck);
    Ok(())
}

#[tauri::command]
pub(crate) fn set_cue_active(
    engine: tauri::State<'_, crate::engine::Engine>,
    midi: tauri::State<'_, crate::midi::MidiState>,
    deck: String,
    active: bool,
) -> Result<(), String> {
    engine.set_cue_active(ParamOrigin::Ui, &deck, active)?;
    crate::midi::refresh_led(&engine, &midi, crate::midi::Feedback::Cue, &deck);
    Ok(())
}

#[tauri::command]
pub(crate) fn set_loop_active(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
    active: bool,
) -> Result<DeckSyncPayload, String> {
    engine.set_loop_active(ParamOrigin::Ui, &deck, active)
}

#[tauri::command]
pub(crate) fn set_loop_in(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
) -> Result<DeckSyncPayload, String> {
    engine.loop_in(ParamOrigin::Ui, &deck)
}

#[tauri::command]
pub(crate) fn set_loop_out(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
) -> Result<Option<LoopOutResult>, String> {
    engine.loop_out(ParamOrigin::Ui, &deck)
}

#[tauri::command]
pub(crate) fn set_reloop(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
) -> Result<DeckSyncPayload, String> {
    engine.reloop(ParamOrigin::Ui, &deck)
}

/// Every deck-scope mixer move, addressed the way the manifest addresses it. An unknown
/// address is ignored, the same way a richer mixer's session replays everything else.
#[tauri::command]
pub(crate) fn set_deck_param(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
    slot: String,
    param: String,
    value: f32,
) -> Result<(), String> {
    engine.set_deck_param(ParamOrigin::Ui, &deck, &slot, &param, value)
}

// Session-view mute/solo. Not logged: it is a monitoring control, not a
// recorded mixer move, and the offline render must not be affected by it.
#[tauri::command]
pub(crate) fn set_deck_muted(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
    muted: bool,
) -> Result<(), String> {
    engine
        .audio
        .strip(&deck)
        .ok_or_else(|| format!("unknown deck: {deck}"))?
        .locked()
        .set_muted(muted);
    Ok(())
}

#[tauri::command]
pub(crate) fn set_fader_curve(
    engine: tauri::State<'_, crate::engine::Engine>,
    curve: session_core::FaderCurve,
) {
    engine.set_fader_curve(curve);
}

#[tauri::command]
pub(crate) fn set_master_gain(engine: tauri::State<'_, crate::engine::Engine>, gain: f32) {
    engine.set_master_gain(gain);
}

#[tauri::command]
pub(crate) fn set_xfader_position(engine: tauri::State<'_, crate::engine::Engine>, position: f32) {
    engine.set_xfader_position(ParamOrigin::Ui, position);
}

#[tauri::command]
pub(crate) fn set_xfader_assign(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: &str,
    assign: &str,
) -> Result<(), String> {
    engine.set_xfader_assign(
        ParamOrigin::Ui,
        deck,
        session_core::XfaderAssign::from_str_or_thru(assign),
    )
}

#[tauri::command]
pub(crate) async fn load_track(
    app: tauri::AppHandle,
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
    path: String,
    analyze: bool,
    beat_offset_sec: f64,
) -> Result<TrackInfo, String> {
    let deck_arc = engine.deck(&deck)?;
    let device_sample_rate = engine.audio.device_sample_rate;
    let bpm_min = engine
        .audio
        .bpm_min
        .load(std::sync::atomic::Ordering::Relaxed) as f64;
    let bpm_max = engine
        .audio
        .bpm_max
        .load(std::sync::atomic::Ordering::Relaxed) as f64;

    let path_for_log = path.clone();

    let stream_deck = Arc::clone(&deck_arc);
    let stream_app = app.clone();
    let stream_deck_id = deck.clone();

    let (samples, channels, bpm, silence_end, native_sr, cover_art, reducer) =
        tokio::task::spawn_blocking(move || -> Result<_, String> {
            let started = std::time::Instant::now();
            let cover_art = audio::read_cover_art(&path);
            let cover_ms = started.elapsed().as_millis();

            let at_decode = std::time::Instant::now();
            // The reduction runs on its own thread: on the decode thread it doubled the
            // time to a playable deck, which is the one thing decode is on the path for.
            let (to_reducer, from_decoder) =
                std::sync::mpsc::channel::<(Vec<f32>, audio::DecodedShape)>();
            let reducer = std::thread::spawn(move || {
                let mut stream: Option<audio::BandStream> = None;
                let mut last_emit = std::time::Instant::now();
                while let Ok((decoded, shape)) = from_decoder.recv() {
                    let reducer = stream.get_or_insert_with(|| {
                        let started = audio::BandStream::new(
                            shape.total_frames,
                            shape.channels,
                            shape.sample_rate,
                            POINTS_PER_CHUNK,
                        );
                        let mut deck_state = stream_deck.locked();
                        deck_state.reset_dense_points(started.total_points());
                        started
                    });
                    reducer.push(&decoded, |points, total| {
                        let ready = {
                            let mut deck_state = stream_deck.locked();
                            deck_state.push_dense_points(points);
                            deck_state.dense_points.len() / 4
                        };
                        if ready == total || last_emit.elapsed() >= PROGRESS_INTERVAL {
                            last_emit = std::time::Instant::now();
                            stream_app
                                .emit(
                                    "waveform-progress",
                                    WaveformProgress {
                                        deck: stream_deck_id.clone(),
                                        points_ready: ready,
                                        total_points: total,
                                        points_per_sec: audio::DENSE_POINTS_PER_SEC,
                                    },
                                )
                                .ok();
                        }
                    });
                }
                stream
            });

            let (raw_samples, channels, native_sr) =
                audio::decode_audio_streaming(&path, |decoded, shape| {
                    // Only when no resampling stands between the decoder and the analysis;
                    // otherwise the points would be indexed at a rate the bands are not.
                    if shape.sample_rate != device_sample_rate || shape.total_frames == 0 {
                        return;
                    }
                    to_reducer.send((decoded.to_vec(), shape)).ok();
                })
                .map_err(|e| e.to_string())?;
            drop(to_reducer);
            let decode_ms = at_decode.elapsed().as_millis();

            let at_analyse = std::time::Instant::now();

            let (bpm, silence_end) = if analyze {
                let mono_owned: Vec<f32>;
                let mono: &[f32] = if channels == 1 {
                    &raw_samples
                } else {
                    mono_owned = raw_samples
                        .chunks(channels)
                        .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
                        .collect();
                    &mono_owned
                };
                (
                    audio::detect_bpm(mono, native_sr, bpm_min, bpm_max),
                    audio::detect_silence_end(mono, native_sr),
                )
            } else {
                log::info!("load_track: skipping analysis (saved data will be used)");
                (None, 0.0)
            };
            let analyse_ms = at_analyse.elapsed().as_millis();

            let at_resample = std::time::Instant::now();
            let resampled = if native_sr == device_sample_rate {
                raw_samples
            } else {
                audio::resample_linear(&raw_samples, channels, native_sr, device_sample_rate)
            };
            let resample_ms = at_resample.elapsed().as_millis();

            log::info!(
                "load_track timing: cover={cover_ms}ms decode={decode_ms}ms analyse={analyse_ms}ms resample={resample_ms}ms total={}ms",
                started.elapsed().as_millis()
            );

            Ok((
                Arc::new(resampled),
                channels,
                bpm,
                silence_end,
                native_sr,
                cover_art,
                // Handed on rather than joined: waiting here would hold the deck unplayable
                // until the analysis finished, which is the whole thing decode is on the
                // path for.
                reducer,
            ))
        })
        .await
        .map_err(|e| e.to_string())??;

    let total_frames = samples.len() / channels;
    let duration = total_frames as f64 / device_sample_rate as f64;
    // main_pos, cue_pos and cue_point start equal, or the first press_cue reads as
    // CueMoved instead of starting a preview.
    let start_pos = (beat_offset_sec * device_sample_rate as f64).clamp(0.0, total_frames as f64);

    log::info!(
        "load_track [{}]: analyze={} native_sr={} device_sr={} channels={} duration={:.2}s bpm={:?} silence_end={:.3}s beat_offset={:.3}s start_pos={:.0} frames",
        deck, analyze, native_sr, device_sample_rate, channels, duration, bpm, silence_end, beat_offset_sec, start_pos
    );

    let loaded_at_frame;
    {
        let mut deck_state = deck_arc.locked();
        loaded_at_frame = deck_state.render_frame();
        deck_state.load(
            &path_for_log,
            Arc::clone(&samples),
            channels,
            device_sample_rate,
        );
        deck_state.open_at(start_pos);
    }

    // Backgrounded so the waveform never holds up the load.
    let deck_id = deck.clone();
    tokio::spawn(async move {
        let at_bands = std::time::Instant::now();
        let progress_deck = deck_id.clone();
        let progress_app = app.clone();
        let progress_arc = Arc::clone(&deck_arc);
        let streamed = tokio::task::spawn_blocking(move || {
            let mut last_emit = std::time::Instant::now();
            let mut on_points = |points: &[f32], total: usize| {
                let ready = {
                    let mut deck_state = progress_arc.locked();
                    deck_state.push_dense_points(points);
                    deck_state.dense_points.len() / 4
                };
                if ready == total || last_emit.elapsed() >= PROGRESS_INTERVAL {
                    last_emit = std::time::Instant::now();
                    progress_app
                        .emit(
                            "waveform-progress",
                            WaveformProgress {
                                deck: progress_deck.clone(),
                                points_ready: ready,
                                total_points: total,
                                points_per_sec: audio::DENSE_POINTS_PER_SEC,
                            },
                        )
                        .ok();
                }
            };
            // Already reduced alongside the decode unless the file needed resampling.
            match reducer.join().unwrap_or(None) {
                Some(stream) => stream.finish(&mut on_points),
                None => {
                    let mut stream = audio::BandStream::new(
                        total_frames,
                        channels,
                        device_sample_rate,
                        POINTS_PER_CHUNK,
                    );
                    {
                        let mut deck_state = progress_arc.locked();
                        deck_state.reset_dense_points(stream.total_points());
                    }
                    stream.push(&samples, &mut on_points);
                    stream.finish(&mut on_points)
                }
            }
        })
        .await
        .unwrap_or_else(|_| audio::StreamedBands {
            bass: Vec::new(),
            mid: Vec::new(),
            high: Vec::new(),
            bass_rms: 0.0,
            mid_rms: 0.0,
            high_rms: 0.0,
        });

        let bands = audio::SpectralBands {
            bass_rms: streamed.bass_rms,
            mid_rms: streamed.mid_rms,
            high_rms: streamed.high_rms,
            bass: Arc::new(streamed.bass),
            mid: Arc::new(streamed.mid),
            high: Arc::new(streamed.high),
        };
        log::info!(
            "bands timing [{deck_id}]: finish={}ms",
            at_bands.elapsed().as_millis()
        );

        {
            let mut deck_state = deck_arc.locked();
            deck_state.set_bands(bands);
        }

        app.emit("bands-ready", BandsReady { deck: deck_id }).ok();
    });

    engine.recorder.log_at(
        loaded_at_frame,
        "load_track",
        serde_json::json!({
            "deck": deck,
            "path": path_for_log,
            "duration": duration,
            "beat_offset_sec": beat_offset_sec,
        }),
    );

    Ok(TrackInfo {
        duration,
        sample_rate: device_sample_rate,
        bpm,
        silence_end,
        cover_art,
    })
}

#[tauri::command]
pub(crate) fn eject_track(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
) -> Result<(), String> {
    engine.eject_track(&deck)
}

// Returns flat [bass_norm, mid_norm, high_norm, amplitude] * num_points as raw
// f32 little-endian bytes. Binary transfer avoids JSON serialization overhead
// that would otherwise cause GC pauses on large waveform loads.
#[tauri::command]
pub(crate) async fn get_dense_points(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
    from_point: usize,
    to_point: usize,
) -> Result<tauri::ipc::Response, String> {
    let deck_arc = engine.deck(&deck)?;
    let bytes = {
        let deck_state = deck_arc.locked();
        let held = deck_state.dense_points.len() / 4;
        let from = from_point.min(held);
        let to = to_point.min(held).max(from);
        deck_state.dense_points[from * 4..to * 4]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect::<Vec<u8>>()
    };
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub(crate) async fn get_spectral_waveform_region(
    engine: tauri::State<'_, crate::engine::Engine>,
    deck: String,
    start_sec: f64,
    end_sec: f64,
    num_points: usize,
) -> Result<tauri::ipc::Response, String> {
    let deck_arc = engine.deck(&deck)?;
    let (samples, channels, bands, device_sr) = {
        let deck_state = deck_arc.locked();
        (
            Arc::clone(&deck_state.samples),
            deck_state.channels,
            deck_state.bands.clone(),
            deck_state.device_sample_rate,
        )
    };
    let floats = tokio::task::spawn_blocking(move || {
        audio::compute_spectral_waveform_region(
            &samples, channels, &bands, device_sr, start_sec, end_sec, num_points,
        )
    })
    .await
    .map_err(|e| e.to_string())?;
    let bytes: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub(crate) async fn read_track_tags(path: String) -> audio::TrackTags {
    tokio::task::spawn_blocking(move || audio::read_tags(&path))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub(crate) async fn analyze_track(
    engine: tauri::State<'_, crate::engine::Engine>,
    path: String,
) -> Result<TrackInfo, String> {
    let bpm_min = engine
        .audio
        .bpm_min
        .load(std::sync::atomic::Ordering::Relaxed) as f64;
    let bpm_max = engine
        .audio
        .bpm_max
        .load(std::sync::atomic::Ordering::Relaxed) as f64;
    tokio::task::spawn_blocking(move || -> Result<TrackInfo, String> {
        let (raw_samples, channels, native_sr) =
            audio::decode_audio(&path).map_err(|e| e.to_string())?;

        let total_frames = raw_samples.len() / channels;
        let duration = total_frames as f64 / native_sr as f64;

        let mono_owned: Vec<f32>;
        let mono: &[f32] = if channels == 1 {
            &raw_samples
        } else {
            mono_owned = raw_samples
                .chunks(channels)
                .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
                .collect();
            &mono_owned
        };

        let bpm = audio::detect_bpm(mono, native_sr, bpm_min, bpm_max);
        let silence_end = audio::detect_silence_end(mono, native_sr);

        Ok(TrackInfo {
            duration,
            sample_rate: native_sr,
            bpm,
            silence_end,
            cover_art: None,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

// Reads the in-memory session cache rather than re-decoding, so refetching at a
// higher resolution while zooming is cheap. Falls back to a decode otherwise.
#[tauri::command]
pub(crate) async fn get_track_amplitude_region(
    engine: tauri::State<'_, crate::engine::Engine>,
    sessions: tauri::State<'_, crate::session_playback::SessionLibrary>,
    path: String,
    start_sec: f64,
    end_sec: f64,
    num_points: usize,
) -> Result<Vec<f32>, String> {
    let sr = engine.audio.device_sample_rate;
    // Shares the session preload's decode rather than starting a competing one.
    let (samples, channels) = crate::session_playback::load_track(
        &sessions.track_cache,
        &sessions.track_loads,
        &sessions.decode_permits,
        &path,
        sr,
    )
    .await
    .ok_or_else(|| format!("could not decode {path}"))?;

    tokio::task::spawn_blocking(move || {
        let start_frame = (start_sec * sr as f64).max(0.0) as usize;
        let end_frame = (end_sec * sr as f64).max(0.0) as usize;
        Ok(audio::compute_amplitude_region(
            &samples,
            channels,
            start_frame,
            end_frame,
            num_points,
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) fn set_pitch_range(engine: tauri::State<'_, crate::engine::Engine>, percent: f64) {
    engine.audio.set_pitch_range_percent(percent);
}

#[tauri::command]
pub(crate) fn list_audio_devices(
    engine: tauri::State<'_, crate::engine::Engine>,
) -> Vec<DeviceInfo> {
    engine.audio.list_devices()
}

#[tauri::command]
pub(crate) fn set_cue_device(
    engine: tauri::State<'_, crate::engine::Engine>,
    device_id: String,
    channel_offset: usize,
) -> Result<(), String> {
    engine.audio.set_cue_device(&device_id, channel_offset)
}

#[tauri::command]
pub(crate) fn set_main_device(
    engine: tauri::State<'_, crate::engine::Engine>,
    device_id: String,
    channel_offset: usize,
) -> Result<(), String> {
    engine.audio.set_main_device(&device_id, channel_offset)
}

#[tauri::command]
pub(crate) fn get_master_level(engine: tauri::State<'_, crate::engine::Engine>) -> [f32; 2] {
    engine.audio.get_master_level()
}

#[tauri::command]
pub(crate) fn get_deck_levels(
    engine: tauri::State<'_, crate::engine::Engine>,
) -> std::collections::HashMap<String, [f32; 2]> {
    engine.audio.get_deck_levels()
}

#[tauri::command]
pub(crate) fn set_cue_mix(engine: tauri::State<'_, crate::engine::Engine>, mix: f32) {
    engine.audio.monitor.set_cue_mix(mix);
}

#[tauri::command]
pub(crate) fn set_limiter_enabled(engine: tauri::State<'_, crate::engine::Engine>, enabled: bool) {
    engine.audio.monitor.set_limiter_enabled(enabled);
}

#[tauri::command]
pub(crate) fn set_buffer_size(
    engine: tauri::State<'_, crate::engine::Engine>,
    frames: u32,
) -> Result<(), String> {
    engine.audio.set_buffer_frames(frames)
}

#[tauri::command]
pub(crate) fn set_bpm_range(engine: tauri::State<'_, crate::engine::Engine>, min: u32, max: u32) {
    engine.audio.set_bpm_range(min, max);
}

#[tauri::command]
pub(crate) fn start_recording(
    engine: tauri::State<'_, crate::engine::Engine>,
    recovery: tauri::State<'_, crate::recovery::Recovery>,
    bit_depth: u16,
    use_flac: bool,
    record_session: bool,
) -> Result<(), String> {
    let audio_file = if use_flac {
        crate::recovery::AUDIO_FLAC
    } else {
        crate::recovery::AUDIO_WAV
    };
    let started = crate::recorder::system_time_to_iso8601(std::time::SystemTime::now());
    let job = recovery.begin(
        crate::recovery::JobKind::Recording,
        &format!("recording-{}", &started[..10.min(started.len())]),
        Some(audio_file),
        record_session.then_some(crate::recovery::SESSION_LOG),
    )?;

    // Armed before anything is logged, so every event is timed against the first
    // captured frame rather than against the JSON written on the way there.
    let anchor = engine
        .audio
        .start_recording(bit_depth, use_flac, job.path(audio_file))?;

    if record_session {
        engine.recorder.start(
            engine.audio.mixer(),
            engine.audio.monitor.capture_start_handle(),
            crate::audio::RenderFrame::from_master_clock(anchor),
            start_events(&engine.audio),
        );
        engine
            .recorder
            .journal_to(job.path(crate::recovery::SESSION_LOG));
    }
    recovery.set_active_recording(job);
    Ok(())
}

/// Polled rather than pushed like `render-progress`, so the audio layer stays
/// free of an app handle.
#[tauri::command]
pub(crate) fn recording_save_progress(
    engine: tauri::State<'_, crate::engine::Engine>,
) -> Option<f64> {
    engine.audio.save_progress()
}

#[tauri::command]
pub(crate) async fn stop_recording(
    engine: tauri::State<'_, crate::engine::Engine>,
) -> Result<String, String> {
    engine.recorder.stop(engine.master_frame());
    engine.recorder.end_journal();
    let audio = Arc::clone(&engine.audio);
    tokio::task::spawn_blocking(move || audio.stop_recording())
        .await
        .map_err(|e| e.to_string())?
}

fn copy_with_progress(
    src: &str,
    dest: &str,
    progress: &std::sync::atomic::AtomicU32,
) -> Result<(), String> {
    use std::io::{Read, Write};
    const CHUNK_BYTES: usize = 1 << 20;

    let mut reader = std::fs::File::open(src).map_err(|e| e.to_string())?;
    let total = reader.metadata().map_err(|e| e.to_string())?.len();
    let mut writer = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut buffer = vec![0u8; CHUNK_BYTES];
    let mut copied: u64 = 0;

    loop {
        let read = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        writer
            .write_all(&buffer[..read])
            .map_err(|e| e.to_string())?;
        copied += read as u64;
        if let Some(permille) = crate::audio::save_permille(copied, total) {
            progress.store(permille, std::sync::atomic::Ordering::Relaxed);
        }
    }
    writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn save_recording(
    engine: tauri::State<'_, crate::engine::Engine>,
    recovery: tauri::State<'_, crate::recovery::Recovery>,
    src: String,
    dest: String,
    write_bms: bool,
    write_cue: bool,
) -> Result<(), String> {
    if std::fs::rename(&src, &dest).is_err() {
        // rename fails across filesystems. Scoped so the dial reads idle again
        // before the log and cue sheet are written, which report nothing.
        let copied = {
            let progress = engine.audio.begin_save();
            let dial = progress.share();
            let (from, to) = (src.clone(), dest.clone());
            tokio::task::spawn_blocking(move || copy_with_progress(&from, &to, &dial))
                .await
                .map_err(|e| e.to_string())?
        };
        copied?;
        if let Err(e) = std::fs::remove_file(&src) {
            eprintln!("save_recording: failed to remove source file {src}: {e}");
        }
    }
    if !write_bms && !write_cue {
        recovery.finish_recording(&src);
        return Ok(());
    }
    let Some(log) = engine.recorder.take_pending() else {
        recovery.finish_recording(&src);
        return Ok(());
    };
    if write_bms {
        let stem = strip_audio_extension(&dest);
        let log_dest = unique_path(&format!("{stem}.bms"));
        if let Err(e) = std::fs::write(&log_dest, log.as_bytes()) {
            eprintln!("save_recording: failed to write session log {log_dest}: {e}");
        }
    }
    if write_cue {
        match serde_json::from_str::<session_core::event::SessionFile>(&log) {
            Ok(session) => write_cue_sheet(&dest, &session.events),
            Err(error) => log::warn!("save_recording: cannot parse log for cue sheet: {error}"),
        }
    }
    recovery.finish_recording(&src);
    Ok(())
}

#[tauri::command]
pub(crate) fn discard_recording(
    engine: tauri::State<'_, crate::engine::Engine>,
    recovery: tauri::State<'_, crate::recovery::Recovery>,
    path: String,
) -> Result<(), String> {
    engine.recorder.take_pending();
    std::fs::remove_file(&path).ok();
    recovery.finish_recording(&path);
    Ok(())
}

#[tauri::command]
pub(crate) fn list_recoverable(
    recovery: tauri::State<'_, crate::recovery::Recovery>,
) -> Vec<crate::recovery::Recoverable> {
    recovery.list()
}

/// One file at a time, so a user who wants the log but not the audio is not made to take
/// both. Moving it out is what retires it: a job holding nothing is swept by `list`.
#[tauri::command]
pub(crate) fn recover_save_file(
    recovery: tauri::State<'_, crate::recovery::Recovery>,
    id: String,
    file: String,
    dest: String,
) -> Result<(), String> {
    recovery.save_file(&id, &file, &dest)
}

#[tauri::command]
pub(crate) fn recover_discard(
    recovery: tauri::State<'_, crate::recovery::Recovery>,
    id: String,
) -> Result<(), String> {
    recovery.discard(&id)
}

#[tauri::command]
pub(crate) fn save_bms_only(
    engine: tauri::State<'_, crate::engine::Engine>,
    src: String,
    dest: String,
) -> Result<(), String> {
    if let Err(e) = std::fs::remove_file(&src) {
        eprintln!("save_bms_only: failed to remove source file {src}: {e}");
    }
    if let Some(log) = engine.recorder.take_pending() {
        std::fs::write(&dest, log.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

static RENDER_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
pub(crate) fn cancel_render() {
    RENDER_CANCEL.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RenderProgress {
    fraction: f64,
    /// The encode runs after the last block and takes seconds on a long set, so it is
    /// announced rather than left as a full bar.
    writing: bool,
}

#[tauri::command]
pub(crate) async fn render_session_to_file(
    app: tauri::AppHandle,
    engine: tauri::State<'_, crate::engine::Engine>,
    sessions: tauri::State<'_, crate::session_playback::SessionLibrary>,
    session_path: String,
    output_path: String,
    use_flac: bool,
    write_cue: bool,
) -> Result<(), String> {
    RENDER_CANCEL.store(false, std::sync::atomic::Ordering::Relaxed);
    let audio_file = if use_flac {
        crate::recovery::AUDIO_FLAC
    } else {
        crate::recovery::AUDIO_WAV
    };
    let job = app.state::<crate::recovery::Recovery>().begin(
        crate::recovery::JobKind::Render,
        strip_audio_extension(
            std::path::Path::new(&output_path)
                .file_name()
                .map_or("render", |name| name.to_str().unwrap_or("render")),
        ),
        Some(audio_file),
        None,
    )?;
    let partial_path = job.path(audio_file);
    // Prefer the in-memory session so unsaved edits are rendered too.
    let cached = sessions.files.locked().get(&session_path).cloned();
    let limiter = if engine.audio.monitor.limiter_enabled() {
        crate::offline_render::MasterLimiter::On
    } else {
        crate::offline_render::MasterLimiter::Off
    };
    tokio::task::spawn_blocking(move || {
        let session = match cached {
            Some(cached_session) => cached_session,
            None => {
                let json = std::fs::read_to_string(&session_path)
                    .map_err(|e| format!("{session_path}: {e}"))?;
                std::sync::Arc::new(
                    session_core::event::SessionFile::parse(&json)
                        .map_err(|e| format!("parse error: {e}"))?,
                )
            }
        };
        let sample_rate = crate::offline_render::recorded_sample_rate(&session).unwrap_or(44_100);
        let rendered = crate::offline_render::render_session_with_progress(
            &session,
            crate::offline_render::RenderRequest {
                sample_rate,
                min_frames: 0,
                limiter,
            },
            &mut |fraction| {
                app.emit(
                    "render-progress",
                    RenderProgress {
                        fraction,
                        writing: false,
                    },
                )
                .ok();
                !RENDER_CANCEL.load(std::sync::atomic::Ordering::Relaxed)
            },
        )?;
        app.emit(
            "render-progress",
            RenderProgress {
                fraction: 1.0,
                writing: true,
            },
        )
        .ok();
        let write_result = if use_flac {
            crate::audio_file::write_flac_f32(&partial_path, &rendered, sample_rate)
        } else {
            crate::audio_file::write_wav_f32(&partial_path, &rendered, sample_rate, 2)
        };
        // Left in the recovery directory on failure rather than deleted: an encode that
        // died still holds most of a render the user waited a long time for.
        write_result?;
        if std::fs::rename(&partial_path, &output_path).is_err() {
            std::fs::copy(&partial_path, &output_path).map_err(|e| e.to_string())?;
            std::fs::remove_file(&partial_path).ok();
        }
        if write_cue {
            write_cue_sheet(&output_path, &session.events);
        }
        job.finish();
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
    // A cancel is something the user asked for, not a failure to report back at them.
    .or_else(|error| {
        if error == crate::offline_render::RENDER_CANCELLED {
            Ok(())
        } else {
            Err(error)
        }
    })
}

#[tauri::command]
pub(crate) fn save_session(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content.as_bytes()).map_err(|e| e.to_string())
}

/// `base_name` comes from the caller because it is translated and carries a
/// formatted date. The extension is the only part this knows about.
#[tauri::command]
pub(crate) async fn pick_save_path(format: String, base_name: String) -> Option<String> {
    let (label, ext) = match format.as_str() {
        "flac" => ("FLAC Audio", "flac"),
        "session" => ("Beatmatcher Session", "bms"),
        _ => ("WAV Audio", "wav"),
    };
    rfd::AsyncFileDialog::new()
        .add_filter(label, &[ext])
        .set_file_name(format!("{base_name}.{ext}"))
        .save_file()
        .await
        .map(|f| f.path().to_string_lossy().into_owned())
}

#[tauri::command]
pub(crate) fn files_info(paths: Vec<String>) -> Vec<Option<u64>> {
    paths
        .into_iter()
        .map(|p| std::fs::metadata(&p).ok().map(|m| m.len()))
        .collect()
}

#[tauri::command]
pub(crate) fn scan_folder(path: String) -> Vec<String> {
    let mut results = Vec::new();
    scan_dir_recursive(std::path::Path::new(&path), &mut results);
    results
}

#[tauri::command]
pub(crate) fn read_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn confirm_quit(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub(crate) async fn open_session_dialog() -> Option<crate::session_playback::OpenedFile> {
    crate::session_playback::open_session_dialog().await
}

#[tauri::command]
pub(crate) async fn preload_session(
    app: tauri::AppHandle,
    engine: tauri::State<'_, crate::engine::Engine>,
    sessions: tauri::State<'_, crate::session_playback::SessionLibrary>,
    path: String,
) -> Result<(), String> {
    crate::session_playback::preload_session(app, engine, sessions, path).await
}

#[tauri::command]
pub(crate) fn unload_session(
    sessions: tauri::State<'_, crate::session_playback::SessionLibrary>,
    path: String,
) {
    crate::session_playback::unload_session(sessions, path)
}

#[tauri::command]
pub(crate) async fn update_session_events(
    engine: tauri::State<'_, crate::engine::Engine>,
    sessions: tauri::State<'_, crate::session_playback::SessionLibrary>,
    path: String,
    events_json: String,
) -> Result<(), String> {
    crate::session_playback::update_session_events(engine, sessions, path, events_json).await
}

#[tauri::command]
pub(crate) async fn start_session_playback(
    app: tauri::AppHandle,
    engine: tauri::State<'_, crate::engine::Engine>,
    sessions: tauri::State<'_, crate::session_playback::SessionLibrary>,
    path: String,
    from_ms: f64,
) -> Result<(), String> {
    crate::session_playback::start_session_playback(app, engine, sessions, path, from_ms).await
}

#[tauri::command]
pub(crate) fn stop_session_playback(
    engine: tauri::State<'_, crate::engine::Engine>,
    sessions: tauri::State<'_, crate::session_playback::SessionLibrary>,
) {
    crate::session_playback::stop_session_playback(engine, sessions)
}

#[tauri::command]
pub(crate) fn list_midi_devices(
    state: tauri::State<'_, crate::midi::MidiState>,
    app_state: tauri::State<'_, crate::engine::Engine>,
) -> Result<Vec<crate::midi::MidiDevice>, String> {
    crate::midi::list_midi_devices(state, app_state)
}

#[tauri::command]
pub(crate) fn set_midi_device_deck(
    state: tauri::State<'_, crate::midi::MidiState>,
    app_state: tauri::State<'_, crate::engine::Engine>,
    port: String,
    deck: Option<String>,
) -> Result<(), String> {
    crate::midi::set_midi_device_deck(state, app_state, port, deck)
}

#[tauri::command]
pub(crate) fn set_midi_monitor(state: tauri::State<'_, crate::midi::MidiState>, enabled: bool) {
    crate::midi::set_midi_monitor(state, enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio::{CuePressOutcome, Deck};

    #[test]
    fn record_start_stamps_the_jog_rotation_speed() {
        for speed in [
            session_core::JogRotationSpeed::Rpm33,
            session_core::JogRotationSpeed::Rpm45,
        ] {
            let (event_type, payload) = jog_speed_start_event(speed);
            assert_eq!(event_type, "set_jog_rotation_speed");
            assert_eq!(payload, serde_json::json!({ "speed": speed.as_str() }));
        }
    }

    #[test]
    fn record_start_stamps_the_crossfader_and_every_assigned_deck() {
        let events = crossfader_start_events(
            -1.0,
            &[
                ("A", session_core::XfaderAssign::A),
                ("B", session_core::XfaderAssign::B),
                ("C", session_core::XfaderAssign::Thru),
            ],
        );

        assert_eq!(
            events,
            vec![
                (
                    "set_param",
                    serde_json::json!({ "slot": "xfader", "param": "position", "value": -1.0 })
                ),
                (
                    "set_xfader_assign",
                    serde_json::json!({ "deck": "A", "assign": "a" })
                ),
                (
                    "set_xfader_assign",
                    serde_json::json!({ "deck": "B", "assign": "b" })
                ),
            ]
        );
    }

    #[test]
    fn a_centred_crossfader_with_no_assigns_stamps_only_its_position() {
        let events = crossfader_start_events(
            0.0,
            &[
                ("A", session_core::XfaderAssign::Thru),
                ("B", session_core::XfaderAssign::Thru),
            ],
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "set_param");
    }

    #[test]
    fn record_start_stamps_a_filter_that_was_already_engaged() {
        let events =
            strip_start_events(&session_core::CLASSIC_3BAND_V2, "A", |slot, param| {
                match (slot, param) {
                    ("filter", "active") => Some(1.0),
                    _ => None,
                }
            });
        assert_eq!(
            events,
            vec![(
                "set_param",
                serde_json::json!({ "deck": "A", "slot": "filter", "param": "active", "value": 1.0 })
            )]
        );
    }

    #[test]
    fn record_start_stamps_every_moved_strip_param() {
        let events =
            strip_start_events(&session_core::CLASSIC_3BAND_V2, "B", |slot, param| {
                match (slot, param) {
                    ("eq", "low") => Some(-6.0),
                    ("filter", "value") => Some(0.4),
                    ("fader", "gain") => Some(0.75),
                    _ => None,
                }
            });
        let moved: Vec<_> = events
            .iter()
            .map(|(_, payload)| (payload["slot"].clone(), payload["param"].clone()))
            .collect();
        assert_eq!(
            moved,
            vec![
                (serde_json::json!("eq"), serde_json::json!("low")),
                (serde_json::json!("filter"), serde_json::json!("value")),
                (serde_json::json!("fader"), serde_json::json!("gain")),
            ]
        );
    }

    #[test]
    fn an_untouched_strip_stamps_nothing() {
        let manifest = &session_core::CLASSIC_3BAND_V2;
        let events = strip_start_events(manifest, "A", |slot, param| {
            manifest
                .strip_slot(slot)
                .and_then(|entry| entry.param(param))
                .map(|descriptor| descriptor.default as f32)
        });
        assert!(events.is_empty(), "{events:?}");
    }

    // Compile-time guard for the SendStream SAFETY contract (audio/stream.rs):
    // making any stream-mutating command async moves cpal::Stream drops onto
    // worker threads, which is UB. An async fn no longer coerces to these.
    #[test]
    fn stream_commands_must_stay_synchronous() {
        let _: fn(tauri::State<'_, crate::engine::Engine>, String, usize) -> Result<(), String> =
            set_main_device;
        let _: fn(tauri::State<'_, crate::engine::Engine>, String, usize) -> Result<(), String> =
            set_cue_device;
        let _: fn(tauri::State<'_, crate::engine::Engine>, u32) -> Result<(), String> =
            set_buffer_size;
    }

    const SR: u32 = 44100;
    const SR_F: f64 = SR as f64;
    const BPM: f64 = 120.0;

    fn beat_dur() -> f64 {
        (60.0 / BPM) * SR_F
    }

    fn deck_with_grid(duration_secs: f64) -> Deck {
        let mut deck_state = Deck::loaded_for_testing(SR, duration_secs);
        deck_state.bpm = Some(BPM);
        deck_state.beat_offset_frames = 0.0;
        deck_state
    }

    #[test]
    fn cue_timecode_is_zero_at_start() {
        assert_eq!(cue_timecode(0.0), "00:00:00");
        assert_eq!(cue_timecode(-100.0), "00:00:00");
    }

    #[test]
    fn cue_timecode_counts_75_frames_per_second() {
        assert_eq!(cue_timecode(1000.0), "00:01:00");
        assert_eq!(cue_timecode(2.0 / 75.0 * 1000.0), "00:00:02");
    }

    #[test]
    fn cue_timecode_rolls_minutes_and_seconds() {
        assert_eq!(cue_timecode(61_000.0), "01:01:00");
        assert_eq!(cue_timecode(3_600_000.0), "60:00:00");
    }

    #[test]
    fn cue_escape_neutralizes_quotes() {
        assert_eq!(cue_escape(r#"A "quoted" title"#), "A 'quoted' title");
    }

    /// Through the real applier, so these cannot pass against a copy of the logic.
    fn simulate_seek(deck_state: &mut Deck, pos: f64) {
        let mut strip = crate::audio::ChannelStrip::from_manifest(crate::audio::MIXER, SR as f32);
        crate::audio::apply_deck_command(
            &session_core::SessionCommand::Seek {
                deck: "A",
                sec: pos / f64::from(SR),
            },
            deck_state,
            &mut strip,
            SR,
            0.0,
            &mut |path: &str| Err(format!("no load in a seek test: {path}")),
        )
        .expect("seek applies");
    }

    #[test]
    fn seek_inside_armed_loop_keeps_loop_active() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.cue_point = beat_dur() * 4.0;
        deck_state.loop_end = beat_dur() * 8.0;
        deck_state.loop_active = true;
        simulate_seek(&mut deck_state, beat_dur() * 6.0);
        assert!(
            deck_state.loop_active,
            "loop must stay armed when seeking inside the region"
        );
    }

    #[test]
    fn seek_before_cue_disarms_loop() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.cue_point = beat_dur() * 4.0;
        deck_state.loop_end = beat_dur() * 8.0;
        deck_state.loop_active = true;
        simulate_seek(&mut deck_state, beat_dur() * 2.0);
        assert!(
            !deck_state.loop_active,
            "loop must be disarmed when seeking before cue"
        );
    }

    #[test]
    fn seek_after_loop_end_disarms_loop() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.cue_point = beat_dur() * 4.0;
        deck_state.loop_end = beat_dur() * 8.0;
        deck_state.loop_active = true;
        simulate_seek(&mut deck_state, beat_dur() * 10.0);
        assert!(
            !deck_state.loop_active,
            "loop must be disarmed when seeking past loop_end"
        );
    }

    #[test]
    fn seek_at_loop_end_disarms_loop() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.cue_point = beat_dur() * 4.0;
        deck_state.loop_end = beat_dur() * 8.0;
        deck_state.loop_active = true;
        simulate_seek(&mut deck_state, beat_dur() * 8.0);
        assert!(
            !deck_state.loop_active,
            "loop_end is exclusive; seeking there must disarm"
        );
    }

    #[test]
    fn seek_inside_disarmed_loop_does_not_rearm() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.cue_point = beat_dur() * 4.0;
        deck_state.loop_end = beat_dur() * 8.0;
        deck_state.loop_active = false;
        simulate_seek(&mut deck_state, beat_dur() * 6.0);
        assert!(
            !deck_state.loop_active,
            "seeking inside a disarmed loop must not rearm it"
        );
    }

    #[test]
    fn seek_never_clears_cue_point_or_loop_end() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.cue_point = beat_dur() * 4.0;
        deck_state.loop_end = beat_dur() * 8.0;
        deck_state.loop_active = true;
        simulate_seek(&mut deck_state, beat_dur() * 1.0);
        assert!(
            (deck_state.cue_point - beat_dur() * 4.0).abs() < 1e-9,
            "cue_point must not move"
        );
        assert!(
            (deck_state.loop_end - beat_dur() * 8.0).abs() < 1e-9,
            "loop_end must not move"
        );
    }

    fn load_deck_at_beat_offset(beat_offset_sec: f64, duration_secs: f64) -> Deck {
        let mut d = Deck::loaded_for_testing(SR, duration_secs);
        let start_pos = (beat_offset_sec * SR_F).clamp(0.0, d.total_frames as f64);
        d.is_playing = false;
        d.is_cueing = false;
        d.main_pos = start_pos;
        d.cue_pos = start_pos;
        d.cue_point = start_pos;
        d
    }

    #[test]
    fn press_cue_starts_preview_immediately_after_load() {
        // After load_track, main_pos == cue_point, so press_cue must return PreviewStarted
        // without a CueMoved step first.
        let mut d = load_deck_at_beat_offset(1.5, 10.0);
        let outcome = d.press_cue();
        assert!(
            matches!(outcome, CuePressOutcome::PreviewStarted),
            "expected PreviewStarted, got {:?}",
            outcome
        );
        assert!(d.is_playing);
        assert!(d.is_cueing);
    }

    #[test]
    fn press_cue_cue_moved_when_main_pos_differs_from_cue_point() {
        let mut d = load_deck_at_beat_offset(0.0, 10.0);
        d.main_pos = 1.5 * SR_F;
        let outcome = d.press_cue();
        assert!(
            matches!(outcome, CuePressOutcome::CueMoved { .. }),
            "expected CueMoved when positions differ, got {:?}",
            outcome
        );
        assert!(!d.is_playing);
    }

    #[test]
    fn press_cue_starts_preview_at_nonzero_beat_offset() {
        let beat_offset_sec = 0.342; // typical silence-skip value
        let mut d = load_deck_at_beat_offset(beat_offset_sec, 10.0);
        let outcome = d.press_cue();
        assert!(
            matches!(outcome, CuePressOutcome::PreviewStarted),
            "expected PreviewStarted at beat_offset={}, got {:?}",
            beat_offset_sec,
            outcome
        );
    }

    #[test]
    fn disarm_then_seek_still_allows_reloop() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.cue_point = beat_dur() * 4.0;
        deck_state.loop_end = beat_dur() * 8.0;
        deck_state.loop_active = true;
        simulate_seek(&mut deck_state, beat_dur() * 2.0);
        assert!(!deck_state.loop_active);
        assert!(
            deck_state.loop_end > deck_state.cue_point,
            "region must still be valid for reloop"
        );
        deck_state.main_pos = deck_state.cue_point;
        deck_state.cue_pos = deck_state.cue_point;
        deck_state.loop_active = true;
        assert!(deck_state.loop_active);
        assert!((deck_state.main_pos - beat_dur() * 4.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod cue_fixture {
    use super::*;
    use crate::offline_render::corpus::CORPUS;

    // Falls back to the file stem, so the expected text is machine independent.
    fn no_tags(_path: &str) -> audio::TrackTags {
        audio::TrackTags::default()
    }

    fn sheet_for(json: &str) -> String {
        let session: session_core::SessionFile =
            serde_json::from_str(json).expect("parse corpus fixture");
        let points = session_core::build_cue_points(&session.events);
        cue_sheet_text("set.wav", "set", &points, no_tags)
    }

    fn sheet_named(name: &str) -> String {
        let (_, json) = CORPUS
            .iter()
            .find(|(id, _)| *id == name)
            .unwrap_or_else(|| panic!("no corpus fixture named {name}"));
        sheet_for(json)
    }

    #[test]
    fn transport_fixture_cue_sheet_is_unchanged() {
        assert_eq!(
            sheet_named("transport"),
            "TITLE \"set\"\n\
             FILE \"set.wav\" WAVE\n\
             \x20 TRACK 01 AUDIO\n\
             \x20   TITLE \"__SOURCE__\"\n\
             \x20   INDEX 01 00:00:08\n"
        );
    }

    #[test]
    fn multideck_fixture_lists_each_deck_once_in_audible_order() {
        assert_eq!(
            sheet_named("rate_and_multideck"),
            "TITLE \"set\"\n\
             FILE \"set.wav\" WAVE\n\
             \x20 TRACK 01 AUDIO\n\
             \x20   TITLE \"__SOURCE__\"\n\
             \x20   INDEX 01 00:00:00\n\
             \x20 TRACK 02 AUDIO\n\
             \x20   TITLE \"__SOURCE__\"\n\
             \x20   INDEX 01 00:00:30\n"
        );
    }

    #[test]
    fn every_corpus_fixture_produces_a_stable_sheet() {
        for (name, json) in CORPUS {
            let sheet = sheet_for(json);
            assert!(
                sheet.starts_with("TITLE \"set\"\nFILE \"set.wav\" WAVE\n"),
                "{name}: header changed"
            );
            assert_eq!(
                sheet,
                sheet_for(json),
                "{name}: sheet generation is not deterministic"
            );
        }
    }
}
