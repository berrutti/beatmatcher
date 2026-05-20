mod audio;

use audio::{AppAudio, ChannelStrip, CuePressOutcome, DeviceInfo, TrackInfo};
use std::sync::Arc;
use tauri::menu::{AboutMetadataBuilder, MenuBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::Emitter;

fn system_time_to_iso8601(system_time: std::time::SystemTime) -> String {
    const SECS_PER_DAY: i64 = 86400;
    const SECS_PER_HOUR: i64 = 3600;
    const SECS_PER_MIN: i64 = 60;
    // Offset from Unix epoch (1970-01-01) to the proleptic Gregorian epoch (0000-03-01).
    const DAYS_TO_GREGORIAN_EPOCH: i64 = 719468;
    const DAYS_PER_400_YEARS: i64 = 146097;
    const DAYS_PER_100_YEARS: i64 = 36524;
    const DAYS_PER_4_YEARS: i64 = 1460;
    const DAYS_PER_YEAR: i64 = 365;

    let elapsed = system_time.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let total_secs = elapsed.as_secs() as i64;
    let milliseconds = elapsed.subsec_millis();

    let time_of_day = total_secs.rem_euclid(SECS_PER_DAY);
    let hours = time_of_day / SECS_PER_HOUR;
    let minutes = (time_of_day % SECS_PER_HOUR) / SECS_PER_MIN;
    let seconds = time_of_day % SECS_PER_MIN;

    let days_since_gregorian_epoch = total_secs.div_euclid(SECS_PER_DAY) + DAYS_TO_GREGORIAN_EPOCH;
    let gregorian_era = days_since_gregorian_epoch.div_euclid(DAYS_PER_400_YEARS);
    let day_of_era = (days_since_gregorian_epoch - gregorian_era * DAYS_PER_400_YEARS) as u64;
    let year_of_era = (day_of_era - day_of_era / DAYS_PER_4_YEARS as u64
        + day_of_era / DAYS_PER_100_YEARS as u64
        - day_of_era / (DAYS_PER_400_YEARS - 1) as u64)
        / DAYS_PER_YEAR as u64;
    let year_raw = year_of_era as i64 + gregorian_era * 400;
    let day_of_year =
        day_of_era - (DAYS_PER_YEAR as u64 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_param = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_param + 2) / 5 + 1;
    let month = if month_param < 10 { month_param + 3 } else { month_param - 9 };
    let year = year_raw + if month <= 2 { 1 } else { 0 };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, month, day, hours, minutes, seconds, milliseconds
    )
}

struct SessionLogger {
    start: std::time::Instant,
    start_wall: std::time::SystemTime,
    events: Vec<serde_json::Value>,
    pending: Option<String>,
}

impl SessionLogger {
    fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
            start_wall: std::time::SystemTime::now(),
            events: Vec::new(),
            pending: None,
        }
    }

    fn log(&mut self, event_type: &str, payload: serde_json::Value) {
        let t = (self.start.elapsed().as_secs_f64() * 1000.0 * 10.0).round() / 10.0;
        let mut obj = serde_json::Map::new();
        obj.insert("elapsed_ms".into(), serde_json::json!(t));
        obj.insert("type".into(), serde_json::json!(event_type));
        if let serde_json::Value::Object(extra) = payload {
            obj.extend(extra);
        }
        self.events.push(serde_json::Value::Object(obj));
    }

    fn stop(&mut self) {
        let started_at = system_time_to_iso8601(self.start_wall);
        let log = serde_json::json!({
            "version": 1,
            "startedAt": started_at,
            "events": std::mem::take(&mut self.events),
        });
        self.pending = serde_json::to_string_pretty(&log).ok();
    }

    fn take_pending(&mut self) -> Option<String> {
        self.pending.take()
    }
}

pub struct AppState {
    pub audio: Arc<AppAudio>,
    session: std::sync::Mutex<Option<SessionLogger>>,
}

impl AppState {
    fn log(&self, event_type: &str, payload: serde_json::Value) {
        if let Some(logger) = self.session.lock().unwrap().as_mut() {
            logger.log(event_type, payload);
        }
    }
}

// Required because AppState contains AppAudio, which contains SendStream.
// See stream.rs for the full safety argument.
unsafe impl Send for AppState {}
unsafe impl Sync for AppState {}

fn band_normalization_scale(band: &[f32]) -> f32 {
    let max = band.iter().map(|&x| x.abs()).fold(0.0f32, f32::max);
    if max > 0.0 { 1.0 / max } else { 1.0 }
}

fn get_deck(
    state: &tauri::State<'_, AppState>,
    deck: &str,
) -> Result<std::sync::Arc<std::sync::Mutex<audio::DeckState>>, String> {
    state.audio.deck(deck).ok_or_else(|| format!("unknown deck: {}", deck))
}

fn get_strip(
    state: &tauri::State<'_, AppState>,
    deck: &str,
) -> Result<std::sync::Arc<std::sync::Mutex<ChannelStrip>>, String> {
    state.audio.strip(deck).ok_or_else(|| format!("unknown deck: {}", deck))
}

fn sec_to_frame(sec: f64, sample_rate: u32, total_frames: usize) -> f64 {
    (sec * sample_rate as f64).clamp(0.0, total_frames as f64)
}

#[tauri::command]
async fn load_track(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    deck: String,
    path: String,
    analyze: bool,
) -> Result<TrackInfo, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let device_sample_rate = state.audio.device_sample_rate;
    let bpm_min = state.audio.bpm_min.load(std::sync::atomic::Ordering::Relaxed) as f64;
    let bpm_max = state.audio.bpm_max.load(std::sync::atomic::Ordering::Relaxed) as f64;

    let path_for_log = path.clone();

    // Run all CPU-heavy work in a single blocking thread.
    let (samples, channels, bpm, silence_end, native_sr, cover_art) =
        tokio::task::spawn_blocking(move || -> Result<_, String> {
            let cover_art = audio::read_cover_art(&path);
            let (raw_samples, channels, native_sr) =
                audio::decode_audio(&path).map_err(|e| e.to_string())?;

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

            let resampled = if native_sr == device_sample_rate {
                raw_samples
            } else {
                audio::resample_linear(&raw_samples, channels, native_sr, device_sample_rate)
            };

            Ok((std::sync::Arc::new(resampled), channels, bpm, silence_end, native_sr, cover_art))
        })
        .await
        .map_err(|e| e.to_string())??;

    let total_frames = samples.len() / channels;
    let duration = total_frames as f64 / device_sample_rate as f64;
    let silence_pos = silence_end * device_sample_rate as f64;

    log::info!(
        "load_track [{}]: analyze={} native_sr={} device_sr={} channels={} duration={:.2}s bpm={:?} silence_end={:.3}s silence_pos={:.0} frames",
        deck, analyze, native_sr, device_sample_rate, channels, duration, bpm, silence_end, silence_pos
    );

    {
        let mut d = deck_arc.lock().unwrap();
        d.samples = Arc::clone(&samples);
        d.channels = channels;
        d.device_sample_rate = device_sample_rate;
        d.total_frames = total_frames;
        d.duration = duration;
        d.loaded_path = Some(path_for_log.clone());
        d.is_playing = false;
        d.main_pos = silence_pos;
        d.cue_pos = silence_pos;
        d.loop_active = false;
        d.loop_start = 0.0;
        d.loop_end = 0.0;
        d.bpm = None;
        d.beat_offset_frames = 0.0;
        d.playback_rate = 1.0;
        d.nudge_factor = 1.0;
        d.bass_band = Arc::new(Vec::new());
        d.mid_band = Arc::new(Vec::new());
        d.high_band = Arc::new(Vec::new());
        d.bass_scale = 1.0;
        d.mid_scale = 1.0;
        d.high_scale = 1.0;
    }

    // Compute spectral bands in background; emit "bands-ready" when done so the
    // frontend can fetch waveform data without blocking track load.
    let deck_id = deck.clone();
    tokio::spawn(async move {
        let (bass_band, mid_band, high_band) = tokio::task::spawn_blocking(move || {
            audio::compute_spectral_bands(&*samples, channels, device_sample_rate)
        })
        .await
        .unwrap_or_else(|_| (Vec::new(), Vec::new(), Vec::new()));

        let bass_scale = band_normalization_scale(&bass_band);
        let mid_scale = band_normalization_scale(&mid_band);
        let high_scale = band_normalization_scale(&high_band);

        {
            let mut d = deck_arc.lock().unwrap();
            d.bass_band = Arc::new(bass_band);
            d.mid_band = Arc::new(mid_band);
            d.high_band = Arc::new(high_band);
            d.bass_scale = bass_scale;
            d.mid_scale = mid_scale;
            d.high_scale = high_scale;
        }

        app.emit("bands-ready", deck_id).ok();
    });

    state.log("load_track", serde_json::json!({
        "deck": deck,
        "path": path_for_log,
        "duration": duration,
    }));

    Ok(TrackInfo {
        duration,
        sample_rate: device_sample_rate,
        bpm,
        silence_end,
        cover_art,
    })
}

#[tauri::command]
fn play(
    state: tauri::State<'_, AppState>,
    deck: String,
    from_sec: Option<f64>,
) -> Result<(), String> {
    let deck_arc = get_deck(&state, &deck)?;
    let mut d = deck_arc.lock().unwrap();
    if let Some(sec) = from_sec {
        let pos = sec_to_frame(sec, d.device_sample_rate, d.total_frames);
        d.main_pos = pos;
        d.cue_pos = pos;
    } else {
        d.cue_pos = d.main_pos;
    }
    log::info!("play [{}]: from_sec={:?} main_pos={:.0}", deck, from_sec, d.main_pos);
    d.is_playing = true;
    Ok(())
}

#[tauri::command]
fn stop(state: tauri::State<'_, AppState>, deck: String) -> Result<(), String> {
    let deck_arc = get_deck(&state, &deck)?;
    deck_arc.lock().unwrap().is_playing = false;
    Ok(())
}

#[tauri::command]
fn press_cue(state: tauri::State<'_, AppState>, deck: String) -> Result<DeckSyncPayload, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let (outcome, payload) = {
        let mut d = deck_arc.lock().unwrap();
        let out = d.press_cue();
        let loop_cleared = matches!(out, CuePressOutcome::CueMoved { .. }) && d.loop_end > d.loop_start;
        if loop_cleared {
            d.loop_active = false;
            d.loop_start = 0.0;
            d.loop_end = 0.0;
        }
        (out, DeckSyncPayload::from_deck(&d, loop_cleared))
    };
    let (event, cue_sec) = match outcome {
        CuePressOutcome::NoTrack => return Ok(payload),
        CuePressOutcome::PreviewStarted => ("cue_preview_start", payload.cue_point_sec),
        CuePressOutcome::CueMoved { new_cue_point_sec } => ("cue_move", new_cue_point_sec),
        CuePressOutcome::StoppedAtCue { cue_point_sec } => ("stopped_at_cue", cue_point_sec),
    };
    state.log(event, serde_json::json!({ "deck": deck, "cue_point_sec": cue_sec }));
    Ok(payload)
}

#[tauri::command]
fn release_cue(state: tauri::State<'_, AppState>, deck: String) -> Result<DeckSyncPayload, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let (was_cueing, payload) = {
        let mut d = deck_arc.lock().unwrap();
        let was = d.is_cueing;
        d.release_cue();
        (was, DeckSyncPayload::from_deck(&d, false))
    };
    if was_cueing {
        state.log("cue_preview_end",
            serde_json::json!({ "deck": deck, "cue_point_sec": payload.cue_point_sec }));
    }
    Ok(payload)
}

#[tauri::command]
fn toggle_play(state: tauri::State<'_, AppState>, deck: String) -> Result<DeckSyncPayload, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let payload = {
        let mut d = deck_arc.lock().unwrap();
        d.toggle_play();
        DeckSyncPayload::from_deck(&d, false)
    };
    state.log(
        if payload.is_playing { "play" } else { "stop" },
        serde_json::json!({ "deck": deck }),
    );
    Ok(payload)
}

#[tauri::command]
fn set_cue_and_stop(state: tauri::State<'_, AppState>, deck: String) -> Result<DeckSyncPayload, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let (was_playing, payload) = {
        let mut d = deck_arc.lock().unwrap();
        let was = d.is_playing;
        d.set_cue_and_stop();
        (was, DeckSyncPayload::from_deck(&d, false))
    };
    if was_playing {
        state.log("cue_set_and_stop",
            serde_json::json!({ "deck": deck, "cue_point_sec": payload.cue_point_sec }));
    }
    Ok(payload)
}

#[tauri::command]
fn stop_at_cue(state: tauri::State<'_, AppState>, deck: String) -> Result<DeckSyncPayload, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let (was_playing, payload) = {
        let mut d = deck_arc.lock().unwrap();
        let was = d.is_playing;
        d.stop_at_cue();
        (was, DeckSyncPayload::from_deck(&d, false))
    };
    if was_playing {
        state.log("stop_at_cue",
            serde_json::json!({ "deck": deck, "cue_point_sec": payload.cue_point_sec }));
    }
    Ok(payload)
}

#[tauri::command]
fn eject_track(state: tauri::State<'_, AppState>, deck: String) -> Result<(), String> {
    let deck_arc = get_deck(&state, &deck)?;
    {
        let mut d = deck_arc.lock().unwrap();
        d.is_playing = false;
        d.is_cueing = false;
        d.samples = Arc::new(Vec::new());
        d.total_frames = 0;
        d.duration = 0.0;
        d.main_pos = 0.0;
        d.cue_pos = 0.0;
        d.cue_point = 0.0;
        d.loop_active = false;
        d.loop_start = 0.0;
        d.loop_end = 0.0;
        d.bpm = None;
        d.beat_offset_frames = 0.0;
        d.loaded_path = None;
    }
    state.log("eject_track", serde_json::json!({ "deck": deck }));
    Ok(())
}

#[tauri::command]
fn seek(
    state: tauri::State<'_, AppState>,
    deck: String,
    sec: f64,
) -> Result<(), String> {
    let deck_arc = get_deck(&state, &deck)?;
    {
        let mut d = deck_arc.lock().unwrap();
        let pos = sec_to_frame(sec, d.device_sample_rate, d.total_frames);
        log::info!("seek [{}]: {:.3}s -> frame {:.0}", deck, sec, pos);
        d.main_pos = pos;
        d.cue_pos = pos;
    }
    state.log("seek", serde_json::json!({ "deck": deck, "sec": sec }));
    Ok(())
}

#[tauri::command]
fn set_loop_region(
    state: tauri::State<'_, AppState>,
    deck: String,
    start_sec: f64,
    end_sec: f64,
) -> Result<(), String> {
    let deck_arc = get_deck(&state, &deck)?;
    let mut d = deck_arc.lock().unwrap();
    let sr = d.device_sample_rate as f64;
    d.loop_start = start_sec * sr;
    d.loop_end = end_sec * sr;
    Ok(())
}

#[tauri::command]
fn set_loop_active(
    state: tauri::State<'_, AppState>,
    deck: String,
    active: bool,
) -> Result<(), String> {
    get_deck(&state, &deck)?.lock().unwrap().loop_active = active;
    if !active {
        state.log("exit_loop", serde_json::json!({ "deck": deck }));
    }
    Ok(())
}

// Returned by all transport commands so the frontend can mirror deck state
// without any branching logic.
#[derive(serde::Serialize)]
struct DeckSyncPayload {
    is_playing: bool,
    is_cueing: bool,
    cue_point_sec: f64,
    position_sec: f64,
    // true when a CUE move cleared the loop region; frontend must discard loopRegion
    loop_region_cleared: bool,
}

impl DeckSyncPayload {
    fn from_deck(d: &audio::DeckState, loop_region_cleared: bool) -> Self {
        let sr = d.device_sample_rate as f64;
        Self {
            is_playing: d.is_playing,
            is_cueing: d.is_cueing,
            cue_point_sec: if sr > 0.0 { d.cue_point / sr } else { 0.0 },
            position_sec: d.position_sec(),
            loop_region_cleared,
        }
    }
}

#[derive(serde::Serialize)]
struct LoopOutResult {
    start_sec: f64,
    end_sec: f64,
    beats: i64,
    // Some when a late quantized press caused an immediate seek; frontend must sync positionCache.
    seek_to_sec: Option<f64>,
}

fn quantize_to_beat(pos_frames: f64, bpm: f64, beat_offset_frames: f64, sr: f64) -> f64 {
    let beat_dur = (60.0 / bpm) * sr;
    let index = ((pos_frames - beat_offset_frames) / beat_dur).round();
    (beat_offset_frames + index * beat_dur).max(0.0)
}

#[tauri::command]
fn set_beat_grid(
    state: tauri::State<'_, AppState>,
    deck: String,
    bpm: f64,
    beat_offset_sec: f64,
) -> Result<(), String> {
    let arc = get_deck(&state, &deck)?;
    {
        let mut d = arc.lock().unwrap();
        d.bpm = Some(bpm);
        d.beat_offset_frames = beat_offset_sec * d.device_sample_rate as f64;
    }
    state.log("set_beat_grid",
        serde_json::json!({ "deck": deck, "bpm": bpm, "beat_offset_sec": beat_offset_sec }));
    Ok(())
}

fn loop_in_core(d: &mut audio::DeckState, quantize: bool) -> Result<f64, String> {
    let sr = d.device_sample_rate as f64;
    let bpm = d.bpm.ok_or("no beat grid set")?;
    let in_frames = if quantize {
        quantize_to_beat(d.main_pos, bpm, d.beat_offset_frames, sr)
    } else {
        d.main_pos
    };
    d.loop_active = false;
    d.loop_start = 0.0;
    d.loop_end = 0.0;
    Ok(in_frames / sr)
}

fn loop_out_core(
    d: &mut audio::DeckState,
    quantize: bool,
    cue_point_sec: Option<f64>,
) -> Result<Option<LoopOutResult>, String> {
    let sr = d.device_sample_rate as f64;
    let bpm = d.bpm.ok_or("no beat grid set")?;
    let out_frames = if quantize {
        quantize_to_beat(d.main_pos, bpm, d.beat_offset_frames, sr)
    } else {
        d.main_pos
    };
    let bar_frames = (4.0 * 60.0 / bpm) * sr;
    let in_frames = if let Some(cue_sec) = cue_point_sec {
        let cue_frames = cue_sec * sr;
        if cue_frames < out_frames { cue_frames } else { (out_frames - bar_frames).max(0.0) }
    } else if d.loop_end > d.loop_start {
        d.loop_start
    } else {
        (out_frames - bar_frames).max(0.0)
    };
    if out_frames <= in_frames {
        return Ok(None);
    }
    d.loop_start = in_frames;
    d.loop_end = out_frames;
    d.loop_active = true;
    // When quantized and pressed late, main_pos has already passed loop_end.
    // Immediately seek to loop_start + overshoot so the next audio callback
    // reads from the compensated position rather than the overshoot.
    let seek_to_sec = if quantize && d.main_pos > out_frames {
        let dur = out_frames - in_frames;
        let overshoot = d.main_pos - out_frames;
        let new_pos = in_frames + overshoot % dur;
        d.main_pos = new_pos;
        Some(new_pos / sr)
    } else {
        None
    };
    let start_sec = in_frames / sr;
    let end_sec = out_frames / sr;
    let beats = ((end_sec - start_sec) * bpm / 60.0).round() as i64;
    Ok(Some(LoopOutResult { start_sec, end_sec, beats, seek_to_sec }))
}

#[tauri::command]
fn set_loop_in(
    state: tauri::State<'_, AppState>,
    deck: String,
    quantize: bool,
) -> Result<f64, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let sec = loop_in_core(&mut deck_arc.lock().unwrap(), quantize)?;
    state.log("loop_in",
        serde_json::json!({ "deck": deck, "cue_sec": sec, "quantized": quantize }));
    Ok(sec)
}

#[tauri::command]
fn set_loop_out(
    state: tauri::State<'_, AppState>,
    deck: String,
    quantize: bool,
    cue_point_sec: Option<f64>,
) -> Result<Option<LoopOutResult>, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let result = loop_out_core(&mut deck_arc.lock().unwrap(), quantize, cue_point_sec)?;
    if let Some(r) = &result {
        state.log("loop_out", serde_json::json!({
            "deck": deck, "start_sec": r.start_sec, "end_sec": r.end_sec,
            "beats": r.beats, "quantized": quantize,
        }));
    }
    Ok(result)
}

#[tauri::command]
fn clear_loop_region(state: tauri::State<'_, AppState>, deck: String) -> Result<(), String> {
    let arc = get_deck(&state, &deck)?;
    let mut d = arc.lock().unwrap();
    d.loop_active = false;
    d.loop_start = 0.0;
    d.loop_end = 0.0;
    Ok(())
}

#[tauri::command]
fn set_volume(
    state: tauri::State<'_, AppState>,
    deck: String,
    gain: f32,
) -> Result<(), String> {
    get_strip(&state, &deck)?.lock().unwrap().set_gain(gain);
    state.log("set_volume",
        serde_json::json!({ "deck": deck, "gain": gain }));
    Ok(())
}

#[tauri::command]
fn set_playback_rate(
    state: tauri::State<'_, AppState>,
    deck: String,
    rate: f64,
) -> Result<(), String> {
    get_deck(&state, &deck)?.lock().unwrap().playback_rate = rate.max(0.1);
    state.log("set_playback_rate",
        serde_json::json!({ "deck": deck, "rate": rate }));
    Ok(())
}

#[tauri::command]
fn set_nudge(
    state: tauri::State<'_, AppState>,
    deck: String,
    percent: f64,
) -> Result<(), String> {
    get_deck(&state, &deck)?.lock().unwrap().nudge_factor = 1.0 + percent / 100.0;
    state.log("set_nudge",
        serde_json::json!({ "deck": deck, "percent": percent }));
    Ok(())
}

#[tauri::command]
fn set_eq(
    state: tauri::State<'_, AppState>,
    deck: String,
    band: String,
    db: f32,
) -> Result<(), String> {
    get_strip(&state, &deck)?.lock().unwrap().set_eq_band(&band, db);
    state.log("set_eq",
        serde_json::json!({ "deck": deck, "band": band, "db": db }));
    Ok(())
}

#[tauri::command]
fn set_filter(
    state: tauri::State<'_, AppState>,
    deck: String,
    value: f32,
) -> Result<(), String> {
    get_strip(&state, &deck)?.lock().unwrap().set_filter(value);
    state.log("set_filter",
        serde_json::json!({ "deck": deck, "value": value }));
    Ok(())
}

#[tauri::command]
fn set_filter_active(
    state: tauri::State<'_, AppState>,
    deck: String,
    active: bool,
) -> Result<(), String> {
    get_strip(&state, &deck)?.lock().unwrap().set_filter_active(active);
    state.log("set_filter_active",
        serde_json::json!({ "deck": deck, "active": active }));
    Ok(())
}

#[tauri::command]
fn get_position(state: tauri::State<'_, AppState>, deck: String) -> Result<f64, String> {
    Ok(get_deck(&state, &deck)?.lock().unwrap().position_sec())
}

#[tauri::command]
fn get_waveform_region(
    state: tauri::State<'_, AppState>,
    deck: String,
    start_sec: f64,
    end_sec: f64,
    num_points: usize,
) -> Result<Vec<f32>, String> {
    let deck_arc = get_deck(&state, &deck)?;
    // Clone the Arc and metadata under the lock, then release immediately.
    // The sample scan happens outside the lock so the audio callback thread
    // is never blocked by waveform rendering.
    let (samples, channels, device_sr) = {
        let d = deck_arc.lock().unwrap();
        (std::sync::Arc::clone(&d.samples), d.channels, d.device_sample_rate)
    };
    Ok(audio::compute_waveform_region(
        &samples,
        channels,
        device_sr,
        start_sec,
        end_sec,
        num_points,
    ))
}

// Returns flat [bass_norm, mid_norm, high_norm, amplitude] * num_points as raw
// f32 little-endian bytes. Binary transfer avoids JSON serialization overhead
// that would otherwise cause GC pauses on large waveform loads.
#[tauri::command]
async fn get_spectral_waveform_region(
    state: tauri::State<'_, AppState>,
    deck: String,
    start_sec: f64,
    end_sec: f64,
    num_points: usize,
) -> Result<tauri::ipc::Response, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let (samples, channels, bass, mid, high, bass_scale, mid_scale, high_scale, device_sr) = {
        let d = deck_arc.lock().unwrap();
        (
            Arc::clone(&d.samples),
            d.channels,
            Arc::clone(&d.bass_band),
            Arc::clone(&d.mid_band),
            Arc::clone(&d.high_band),
            d.bass_scale,
            d.mid_scale,
            d.high_scale,
            d.device_sample_rate,
        )
    };
    let floats = tokio::task::spawn_blocking(move || {
        audio::compute_spectral_waveform_region(
            &samples, channels, &bass, &mid, &high,
            device_sr, bass_scale, mid_scale, high_scale,
            start_sec, end_sec, num_points,
        )
    }).await.map_err(|e| e.to_string())?;
    let bytes: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
fn set_reloop(state: tauri::State<'_, AppState>, deck: String) -> Result<(), String> {
    let deck_arc = get_deck(&state, &deck)?;
    {
        let mut d = deck_arc.lock().unwrap();
        if d.loop_end <= d.loop_start {
            return Ok(());
        }
        let start = d.loop_start;
        d.main_pos = start;
        d.cue_pos = start;
        if d.is_playing {
            d.loop_active = true;
        }
    }
    state.log("reloop", serde_json::json!({ "deck": deck }));
    Ok(())
}

#[tauri::command]
fn set_cue_active(
    state: tauri::State<'_, AppState>,
    deck: String,
    active: bool,
) -> Result<(), String> {
    get_strip(&state, &deck)?.lock().unwrap().cue_active = active;
    state.log("set_cue_active",
        serde_json::json!({ "deck": deck, "active": active }));
    Ok(())
}

#[tauri::command]
fn list_audio_devices(state: tauri::State<'_, AppState>) -> Vec<DeviceInfo> {
    state.audio.list_devices()
}

#[tauri::command]
fn set_cue_device(
    state: tauri::State<'_, AppState>,
    device_id: String,
    channel_offset: usize,
) -> Result<(), String> {
    state.audio.set_cue_device(&device_id, channel_offset)
}

#[tauri::command]
fn set_main_device(
    state: tauri::State<'_, AppState>,
    device_id: String,
    channel_offset: usize,
) -> Result<(), String> {
    state.audio.set_main_device(&device_id, channel_offset)
}

#[tauri::command]
async fn open_file_dialog() -> Option<String> {
    let result = rfd::AsyncFileDialog::new()
        .add_filter(
            "Audio",
            &["mp3", "wav", "flac", "aac", "ogg", "m4a", "aiff", "opus"],
        )
        .pick_file()
        .await;
    result.map(|f| f.path().to_string_lossy().into_owned())
}

#[derive(serde::Serialize)]
struct SessionFile {
    path: String,
    content: String,
}

#[tauri::command]
async fn open_session_dialog() -> Option<SessionFile> {
    let handle = rfd::AsyncFileDialog::new()
        .add_filter("Session", &["json"])
        .pick_file()
        .await?;
    let content = std::fs::read_to_string(handle.path()).ok()?;
    Some(SessionFile {
        path: handle.path().to_string_lossy().into_owned(),
        content,
    })
}

#[tauri::command]
async fn pick_save_path(format: String) -> Option<String> {
    let (label, ext, name) = if format == "flac" {
        ("FLAC Audio", "flac", "mix.flac")
    } else {
        ("WAV Audio", "wav", "mix.wav")
    };
    rfd::AsyncFileDialog::new()
        .add_filter(label, &[ext])
        .set_file_name(name)
        .save_file()
        .await
        .map(|f| f.path().to_string_lossy().into_owned())
}

#[tauri::command]
fn get_master_level(state: tauri::State<'_, AppState>) -> [f32; 2] {
    state.audio.get_master_level()
}

#[tauri::command]
fn get_deck_levels(state: tauri::State<'_, AppState>) -> std::collections::HashMap<String, [f32; 2]> {
    state.audio.get_deck_levels()
}

#[tauri::command]
fn start_recording(state: tauri::State<'_, AppState>, bit_depth: u16, use_flac: bool, record_session: bool) -> Result<(), String> {
    {
        let mut session = state.session.lock().unwrap();
        *session = if record_session { Some(SessionLogger::new()) } else { None };
        if let Some(logger) = session.as_mut() {
            logger.log("recording_start", serde_json::json!({}));
            for deck_id in ["A", "B", "C", "D"] {
                let Some(arc) = state.audio.deck(deck_id) else { continue };
                let d = arc.lock().unwrap();
                let Some(ref path) = d.loaded_path else { continue };
                logger.log("deck_snapshot", serde_json::json!({
                    "deck": deck_id,
                    "path": path,
                    "position_sec": d.main_pos / d.device_sample_rate as f64,
                    "cue_point_sec": d.cue_point / d.device_sample_rate as f64,
                    "is_playing": d.is_playing,
                    "bpm": d.bpm,
                    "playback_rate": d.playback_rate,
                    "loop_active": d.loop_active,
                    "loop_start_sec": d.loop_start / d.device_sample_rate as f64,
                    "loop_end_sec": d.loop_end / d.device_sample_rate as f64,
                }));
            }
        }
    }
    state.audio.start_recording(bit_depth, use_flac)
}

#[tauri::command]
async fn stop_recording(state: tauri::State<'_, AppState>) -> Result<String, String> {
    {
        let mut session = state.session.lock().unwrap();
        if let Some(logger) = session.as_mut() {
            logger.log("recording_stop", serde_json::json!({}));
            logger.stop();
        }
    }
    let audio = Arc::clone(&state.audio);
    tokio::task::spawn_blocking(move || audio.stop_recording())
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn read_track_tags(path: String) -> audio::TrackTags {
    tokio::task::spawn_blocking(move || audio::read_tags(&path))
        .await
        .unwrap_or(audio::TrackTags { title: None, artist: None })
}

#[tauri::command]
fn save_recording(state: tauri::State<'_, AppState>, src: String, dest: String) -> Result<(), String> {
    if std::fs::rename(&src, &dest).is_err() {
        // rename fails across filesystems; fall back to copy then delete
        std::fs::copy(&src, &dest).map_err(|e| e.to_string())?;
        std::fs::remove_file(&src).ok();
    }
    if let Some(log) = state.session.lock().unwrap().as_mut().and_then(|l| l.take_pending()) {
        let stem = dest.strip_suffix(".wav")
            .or_else(|| dest.strip_suffix(".WAV"))
            .or_else(|| dest.strip_suffix(".flac"))
            .or_else(|| dest.strip_suffix(".FLAC"))
            .unwrap_or(&dest);
        let log_dest = format!("{}.session.json", stem);
        std::fs::write(&log_dest, log.as_bytes()).ok();
    }
    Ok(())
}

#[tauri::command]
fn discard_recording(state: tauri::State<'_, AppState>, path: String) -> Result<(), String> {
    state.session.lock().unwrap().as_mut().and_then(|l| l.take_pending());
    std::fs::remove_file(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_master_gain(state: tauri::State<'_, AppState>, gain: f32) {
    state.audio.monitor.set_master_gain(gain);
    state.log("set_master_gain", serde_json::json!({ "gain": gain }));
}

#[tauri::command]
fn set_cue_mix(state: tauri::State<'_, AppState>, mix: f32) {
    state.audio.monitor.set_cue_mix(mix);
}

#[tauri::command]
fn set_limiter_enabled(state: tauri::State<'_, AppState>, enabled: bool) {
    state.audio.monitor.set_limiter_enabled(enabled);
}

#[tauri::command]
fn set_buffer_size(state: tauri::State<'_, AppState>, frames: u32) -> Result<(), String> {
    state.audio.set_buffer_frames(frames)
}

#[tauri::command]
fn set_bpm_range(state: tauri::State<'_, AppState>, min: u32, max: u32) {
    state.audio.set_bpm_range(min, max);
}

#[tauri::command]
fn files_info(paths: Vec<String>) -> Vec<Option<u64>> {
    paths
        .into_iter()
        .map(|p| std::fs::metadata(&p).ok().map(|m| m.len()))
        .collect()
}

fn scan_dir_recursive(dir: &std::path::Path, results: &mut Vec<String>) {
    const AUDIO_EXT: &[&str] = &["mp3", "wav", "flac", "aac", "ogg", "m4a", "aif", "aiff"];
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<_> = entries.flatten().collect();
    paths.sort_by_key(|e| e.file_name());
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

#[tauri::command]
fn scan_folder(path: String) -> Vec<String> {
    let mut results = Vec::new();
    scan_dir_recursive(std::path::Path::new(&path), &mut results);
    results
}

#[tauri::command]
async fn analyze_track(state: tauri::State<'_, AppState>, path: String) -> Result<TrackInfo, String> {
    let bpm_min = state.audio.bpm_min.load(std::sync::atomic::Ordering::Relaxed) as f64;
    let bpm_max = state.audio.bpm_max.load(std::sync::atomic::Ordering::Relaxed) as f64;
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

        Ok(TrackInfo { duration, sample_rate: native_sr, bpm, silence_end, cover_art: None })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let verbose = std::env::args().any(|a| a == "--verbose")
        || std::env::var("BEATMATCHER_VERBOSE").is_ok();
    let app_level = if verbose {
        log::LevelFilter::Info
    } else {
        log::LevelFilter::Warn
    };

    let audio = AppAudio::new().expect("failed to initialize audio engine");
    let ended_flags: Vec<(String, Arc<std::sync::atomic::AtomicBool>)> = audio
        .ended_flags
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let app_state = AppState {
        audio: Arc::new(audio),
        session: std::sync::Mutex::new(None),
    };

    tauri::Builder::default()
        .manage(app_state)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(move |app| {
            let icon = app.default_window_icon().cloned();
            let about = AboutMetadataBuilder::new()
                .name(Some("beatmatcher"))
                .copyright(Some("Copyright 2025 Matias Berrutti\ngithub.com/berrutti/beatmatcher"))
                .icon(icon)
                .build();
            let app_menu = SubmenuBuilder::new(app, "beatmatcher")
                .item(&PredefinedMenuItem::about(app, None, Some(about))?)
                .separator()
                .item(&PredefinedMenuItem::hide(app, None)?)
                .item(&PredefinedMenuItem::hide_others(app, None)?)
                .separator()
                .item(&PredefinedMenuItem::quit(app, None)?)
                .build()?;
            let menu = MenuBuilder::new(app).item(&app_menu).build()?;
            app.set_menu(menu)?;

            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(app_level)
                    .level_for("symphonia_format_riff", log::LevelFilter::Warn)
                    .level_for("symphonia_format_isomp4", log::LevelFilter::Warn)
                    .level_for("symphonia_metadata", log::LevelFilter::Warn)
                    .level_for("symphonia_bundle_mp3", log::LevelFilter::Warn)
                    .build(),
            )?;
            let handle = app.handle().clone();
            let flags = ended_flags;
            tauri::async_runtime::spawn(async move {
                let mut interval =
                    tokio::time::interval(tokio::time::Duration::from_millis(100));
                loop {
                    interval.tick().await;
                    for (id, flag) in &flags {
                        if flag.swap(false, std::sync::atomic::Ordering::AcqRel) {
                            handle.emit("track-ended", id.clone()).ok();
                        }
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_track,
            play,
            stop,
            press_cue,
            release_cue,
            toggle_play,
            set_cue_and_stop,
            stop_at_cue,
            eject_track,
            seek,
            set_loop_region,
            set_loop_active,
            set_beat_grid,
            set_loop_in,
            set_loop_out,
            clear_loop_region,
            set_volume,
            set_playback_rate,
            set_nudge,
            set_eq,
            set_filter,
            set_filter_active,
            get_position,
            get_waveform_region,
            get_spectral_waveform_region,
            set_reloop,
            set_cue_active,
            list_audio_devices,
            set_cue_device,
            set_main_device,
            open_file_dialog,
            open_session_dialog,
            pick_save_path,
            files_info,
            scan_folder,
            analyze_track,
            get_master_level,
            get_deck_levels,
            start_recording,
            stop_recording,
            read_track_tags,
            save_recording,
            discard_recording,
            set_master_gain,
            set_cue_mix,
            set_limiter_enabled,
            set_buffer_size,
            set_bpm_range,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio::DeckState;

    const SR: u32 = 44100;
    const SR_F: f64 = 44100.0;
    const BPM: f64 = 120.0;

    fn beat_dur() -> f64 {
        (60.0 / BPM) * SR_F
    }

    fn deck_with_grid(duration_secs: f64) -> DeckState {
        let mut d = DeckState::loaded_for_testing(SR, duration_secs);
        d.bpm = Some(BPM);
        d.beat_offset_frames = 0.0;
        d
    }

    // --- sec_to_frame ---

    #[test]
    fn sec_to_frame_converts_seconds_to_samples() {
        let result = sec_to_frame(1.0, 44100, 100_000);
        assert!((result - 44100.0).abs() < 1e-9);
    }

    #[test]
    fn sec_to_frame_clamps_negative_to_zero() {
        assert_eq!(sec_to_frame(-5.0, 44100, 100_000), 0.0);
    }

    #[test]
    fn sec_to_frame_clamps_at_total_frames() {
        let total = 44100usize;
        assert_eq!(sec_to_frame(100.0, 44100, total), total as f64);
    }

    #[test]
    fn sec_to_frame_fractional_seconds() {
        let result = sec_to_frame(0.5, 44100, 100_000);
        assert!((result - 22050.0).abs() < 1e-9);
    }

    // --- quantize_to_beat ---

    #[test]
    fn quantize_to_beat_snaps_to_exact_beat_boundary() {
        let pos = 3.0 * beat_dur();
        let result = quantize_to_beat(pos, BPM, 0.0, SR_F);
        assert!((result - pos).abs() < 1.0, "got {}", result);
    }

    #[test]
    fn quantize_to_beat_rounds_to_nearest_beat() {
        let pos = 2.0 * beat_dur() + beat_dur() * 0.3;
        let result = quantize_to_beat(pos, BPM, 0.0, SR_F);
        assert!((result - 2.0 * beat_dur()).abs() < 1.0, "got {}", result);
    }

    #[test]
    fn quantize_to_beat_rounds_up_past_midpoint() {
        let pos = 2.0 * beat_dur() + beat_dur() * 0.7;
        let result = quantize_to_beat(pos, BPM, 0.0, SR_F);
        assert!((result - 3.0 * beat_dur()).abs() < 1.0, "got {}", result);
    }

    #[test]
    fn quantize_to_beat_never_returns_negative() {
        let result = quantize_to_beat(1.0, 120.0, 0.0, SR_F);
        assert!(result >= 0.0, "got {}", result);
    }

    #[test]
    fn quantize_to_beat_respects_beat_offset() {
        let offset = beat_dur() * 0.25;
        let pos = offset + beat_dur();
        let result = quantize_to_beat(pos, BPM, offset, SR_F);
        assert!((result - pos).abs() < 1.0, "got {}", result);
    }

    // --- loop_in_core ---

    #[test]
    fn loop_in_returns_current_position_when_not_quantized() {
        let mut d = deck_with_grid(10.0);
        d.main_pos = beat_dur() * 2.7; // between beats
        let sec = loop_in_core(&mut d, false).unwrap();
        assert!((sec - d.main_pos / SR_F).abs() < 1e-9);
    }

    #[test]
    fn loop_in_snaps_to_beat_when_quantized() {
        let mut d = deck_with_grid(10.0);
        d.main_pos = beat_dur() * 2.3; // 30% into beat 2 → should snap to beat 2
        let sec = loop_in_core(&mut d, true).unwrap();
        let expected = 2.0 * beat_dur() / SR_F;
        assert!((sec - expected).abs() < 1e-3, "expected ~{:.4} got {:.4}", expected, sec);
    }

    #[test]
    fn loop_in_clears_existing_loop_region() {
        let mut d = deck_with_grid(10.0);
        d.loop_start = beat_dur();
        d.loop_end = beat_dur() * 3.0;
        d.loop_active = true;
        d.main_pos = beat_dur() * 4.0;
        loop_in_core(&mut d, false).unwrap();
        assert!(!d.loop_active);
        assert_eq!(d.loop_start, 0.0);
        assert_eq!(d.loop_end, 0.0);
    }

    #[test]
    fn loop_in_fails_without_beat_grid() {
        let mut d = DeckState::loaded_for_testing(SR, 10.0);
        // bpm is None by default
        let result = loop_in_core(&mut d, true);
        assert!(result.is_err());
    }

    // --- loop_out_core ---

    #[test]
    fn loop_out_creates_region_using_cue_point_as_start() {
        let mut d = deck_with_grid(10.0);
        d.main_pos = beat_dur() * 4.0;
        let cue_sec = beat_dur() * 2.0 / SR_F;
        let result = loop_out_core(&mut d, false, Some(cue_sec)).unwrap().unwrap();
        assert!((result.start_sec - cue_sec).abs() < 1e-6);
        assert!((result.end_sec - beat_dur() * 4.0 / SR_F).abs() < 1e-6);
        assert!(d.loop_active);
    }

    #[test]
    fn loop_out_falls_back_to_one_bar_when_cue_is_past_out() {
        let mut d = deck_with_grid(10.0);
        d.main_pos = beat_dur() * 2.0;
        // cue_point is after the out point → one-bar fallback
        let cue_sec = beat_dur() * 3.0 / SR_F;
        let result = loop_out_core(&mut d, false, Some(cue_sec)).unwrap().unwrap();
        let bar_sec = 4.0 * 60.0 / BPM;
        let expected_start = (beat_dur() * 2.0 / SR_F - bar_sec).max(0.0);
        assert!((result.start_sec - expected_start).abs() < 1e-6);
    }

    #[test]
    fn loop_out_quantized_snaps_end_to_beat() {
        let mut d = deck_with_grid(10.0);
        d.main_pos = beat_dur() * 4.0 + beat_dur() * 0.1; // 10% past beat 4
        let cue_sec = 0.0;
        let result = loop_out_core(&mut d, true, Some(cue_sec)).unwrap().unwrap();
        let expected_end = beat_dur() * 4.0 / SR_F;
        assert!((result.end_sec - expected_end).abs() < 1e-3);
    }

    #[test]
    fn loop_out_quantized_compensates_late_press_position() {
        let mut d = deck_with_grid(10.0);
        let out_beat = beat_dur() * 4.0;
        // Position is 5% past the loop end beat → late press
        d.main_pos = out_beat + beat_dur() * 0.05;
        let cue_sec = 0.0;
        let result = loop_out_core(&mut d, true, Some(cue_sec)).unwrap().unwrap();
        assert!(result.seek_to_sec.is_some(), "expected seek compensation for late press");
        let compensated = result.seek_to_sec.unwrap();
        // Must be within the loop region
        assert!(compensated >= result.start_sec);
        assert!(compensated < result.end_sec);
        // DeckState main_pos must also be updated
        assert!((d.main_pos - compensated * SR_F).abs() < 1.0);
    }

    #[test]
    fn loop_out_returns_none_when_out_is_before_or_equal_to_start() {
        let mut d = deck_with_grid(10.0);
        // Position at 0 with cue also at 0: out == in → no region
        d.main_pos = 0.0;
        let result = loop_out_core(&mut d, false, Some(0.0)).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn loop_out_beat_count_matches_region_duration() {
        let mut d = deck_with_grid(10.0);
        d.main_pos = beat_dur() * 4.0; // exactly 4 beats from 0
        let cue_sec = 0.0;
        let result = loop_out_core(&mut d, false, Some(cue_sec)).unwrap().unwrap();
        assert_eq!(result.beats, 4);
    }

    // --- DeckState tick: loop wrap-around ---

    #[test]
    fn deck_wraps_within_loop_on_tick() {
        let mut d = DeckState::loaded_for_testing(SR, 10.0);
        d.loop_start = beat_dur();
        d.loop_end = beat_dur() * 2.0;
        d.loop_active = true;
        d.is_playing = true;
        // Start 1 frame before the loop end
        d.main_pos = d.loop_end - 1.0;
        // One tick should wrap to loop_start (+ 0 overshoot from the step of 1.0)
        d.main_tick();
        assert!(
            d.main_pos >= d.loop_start && d.main_pos < d.loop_end,
            "expected position inside loop, got {}",
            d.main_pos
        );
    }

    #[test]
    fn deck_stops_at_natural_end_of_track() {
        let frames = (SR as f64 * 1.0) as usize; // 1-second track
        let mut d = DeckState::loaded_for_testing(SR, 1.0);
        d.is_playing = true;
        // Advance to the last sample
        d.main_pos = (frames - 1) as f64;
        d.main_tick();
        assert!(!d.is_playing, "deck should have stopped");
    }

    #[test]
    fn deck_is_silent_when_not_playing() {
        let mut d = DeckState::loaded_for_testing(SR, 5.0);
        d.is_playing = false;
        let (l, r) = d.main_tick();
        assert_eq!(l, 0.0);
        assert_eq!(r, 0.0);
    }

    #[test]
    fn deck_position_advances_while_playing() {
        let mut d = DeckState::loaded_for_testing(SR, 5.0);
        d.is_playing = true;
        d.main_pos = 0.0;
        for _ in 0..1000 {
            d.main_tick();
        }
        assert!(d.main_pos > 900.0, "expected position to advance, got {}", d.main_pos);
    }

    // --- system_time_to_iso8601 ---

    fn unix_secs(secs: u64) -> std::time::SystemTime {
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs)
    }

    fn unix_millis(ms: u64) -> std::time::SystemTime {
        std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms)
    }

    #[test]
    fn iso8601_unix_epoch_is_midnight_1970() {
        assert_eq!(system_time_to_iso8601(unix_secs(0)), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn iso8601_known_timestamp() {
        // 2026-05-19T12:34:56Z = 1_779_194_096 seconds since epoch
        assert_eq!(
            system_time_to_iso8601(unix_secs(1_779_194_096)),
            "2026-05-19T12:34:56.000Z"
        );
    }

    #[test]
    fn iso8601_milliseconds_preserved() {
        // 1_000 ms = 1 second; add 123 ms
        assert_eq!(
            system_time_to_iso8601(unix_millis(1_000_123)),
            "1970-01-01T00:16:40.123Z"
        );
    }

    #[test]
    fn iso8601_leap_year_feb_29() {
        // 2000-02-29T00:00:00Z = 951_782_400
        assert_eq!(
            system_time_to_iso8601(unix_secs(951_782_400)),
            "2000-02-29T00:00:00.000Z"
        );
    }

    #[test]
    fn iso8601_end_of_year() {
        // 1999-12-31T23:59:59Z = 946_684_799
        assert_eq!(
            system_time_to_iso8601(unix_secs(946_684_799)),
            "1999-12-31T23:59:59.000Z"
        );
    }

    // --- SessionLogger ---

    #[test]
    fn session_logger_records_events_in_order() {
        let mut logger = SessionLogger::new();
        logger.log("first", serde_json::json!({}));
        logger.log("second", serde_json::json!({}));
        logger.log("third", serde_json::json!({}));
        assert_eq!(logger.events.len(), 3);
        assert_eq!(logger.events[0]["type"], "first");
        assert_eq!(logger.events[1]["type"], "second");
        assert_eq!(logger.events[2]["type"], "third");
    }

    #[test]
    fn session_logger_merges_payload_fields() {
        let mut logger = SessionLogger::new();
        logger.log("play", serde_json::json!({ "deck": "A", "rate": 1.0 }));
        let ev = &logger.events[0];
        assert_eq!(ev["type"], "play");
        assert_eq!(ev["deck"], "A");
        assert_eq!(ev["rate"], 1.0);
    }

    #[test]
    fn session_logger_timestamps_are_non_negative() {
        let mut logger = SessionLogger::new();
        logger.log("e", serde_json::json!({}));
        let t = logger.events[0]["elapsed_ms"].as_f64().unwrap();
        assert!(t >= 0.0);
    }

    #[test]
    fn session_logger_stop_produces_valid_json() {
        let mut logger = SessionLogger::new();
        logger.log("recording_start", serde_json::json!({}));
        logger.stop();
        let pending = logger.take_pending().expect("stop should produce pending JSON");
        let parsed: serde_json::Value = serde_json::from_str(&pending).expect("valid JSON");
        assert_eq!(parsed["version"], 1);
        assert!(parsed["startedAt"].as_str().is_some());
        assert!(parsed["events"].is_array());
        assert_eq!(parsed["events"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn session_logger_take_pending_clears_state() {
        let mut logger = SessionLogger::new();
        logger.stop();
        assert!(logger.take_pending().is_some());
        assert!(logger.take_pending().is_none());
    }

    #[test]
    fn session_logger_stop_clears_events() {
        let mut logger = SessionLogger::new();
        logger.log("a", serde_json::json!({}));
        logger.log("b", serde_json::json!({}));
        logger.stop();
        assert!(logger.events.is_empty());
    }
}
