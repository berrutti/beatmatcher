mod audio;

use audio::{AppAudio, ChannelStrip, DeviceInfo, TrackInfo};
use std::sync::Arc;
use tauri::menu::{AboutMetadataBuilder, MenuBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::Emitter;

pub struct AppState {
    pub audio: Arc<AppAudio>,
}

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
fn seek(
    state: tauri::State<'_, AppState>,
    deck: String,
    sec: f64,
) -> Result<(), String> {
    let deck_arc = get_deck(&state, &deck)?;
    let mut d = deck_arc.lock().unwrap();
    let pos = sec_to_frame(sec, d.device_sample_rate, d.total_frames);
    log::info!("seek [{}]: {:.3}s -> frame {:.0}", deck, sec, pos);
    d.main_pos = pos;
    d.cue_pos = pos;
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
    Ok(())
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
    let mut d = arc.lock().unwrap();
    d.bpm = Some(bpm);
    d.beat_offset_frames = beat_offset_sec * d.device_sample_rate as f64;
    Ok(())
}

#[tauri::command]
fn set_loop_in(
    state: tauri::State<'_, AppState>,
    deck: String,
    quantize: bool,
) -> Result<f64, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let mut d = deck_arc.lock().unwrap();
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

#[tauri::command]
fn set_loop_out(
    state: tauri::State<'_, AppState>,
    deck: String,
    quantize: bool,
    cue_point_sec: Option<f64>,
) -> Result<Option<LoopOutResult>, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let mut d = deck_arc.lock().unwrap();
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
    Ok(())
}

#[tauri::command]
fn set_playback_rate(
    state: tauri::State<'_, AppState>,
    deck: String,
    rate: f64,
) -> Result<(), String> {
    get_deck(&state, &deck)?.lock().unwrap().playback_rate = rate.max(0.1);
    Ok(())
}

#[tauri::command]
fn set_nudge(
    state: tauri::State<'_, AppState>,
    deck: String,
    percent: f64,
) -> Result<(), String> {
    get_deck(&state, &deck)?.lock().unwrap().nudge_factor = 1.0 + percent / 100.0;
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
    Ok(())
}

#[tauri::command]
fn set_filter(
    state: tauri::State<'_, AppState>,
    deck: String,
    value: f32,
) -> Result<(), String> {
    get_strip(&state, &deck)?.lock().unwrap().set_filter(value);
    Ok(())
}

#[tauri::command]
fn set_filter_active(
    state: tauri::State<'_, AppState>,
    deck: String,
    active: bool,
) -> Result<(), String> {
    get_strip(&state, &deck)?.lock().unwrap().set_filter_active(active);
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
    Ok(())
}

#[tauri::command]
fn set_cue_active(
    state: tauri::State<'_, AppState>,
    deck: String,
    active: bool,
) -> Result<(), String> {
    get_strip(&state, &deck)?.lock().unwrap().cue_active = active;
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
fn start_recording(state: tauri::State<'_, AppState>, bit_depth: u16, use_flac: bool) -> Result<(), String> {
    state.audio.start_recording(bit_depth, use_flac)
}

#[tauri::command]
async fn stop_recording(state: tauri::State<'_, AppState>) -> Result<String, String> {
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
fn save_recording(src: String, dest: String) -> Result<(), String> {
    if std::fs::rename(&src, &dest).is_ok() {
        return Ok(());
    }
    // rename fails across filesystems; fall back to copy then delete
    std::fs::copy(&src, &dest).map_err(|e| e.to_string())?;
    std::fs::remove_file(&src).ok();
    Ok(())
}


#[tauri::command]
fn discard_recording(path: String) -> Result<(), String> {
    std::fs::remove_file(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_master_gain(state: tauri::State<'_, AppState>, gain: f32) {
    state.audio.monitor.set_master_gain(gain);
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
        let sr = 44100.0f64;
        let bpm = 120.0f64;
        let beat_dur = (60.0 / bpm) * sr; // 22050 frames
        let pos = 3.0 * beat_dur;
        let result = quantize_to_beat(pos, bpm, 0.0, sr);
        assert!((result - pos).abs() < 1.0, "got {}", result);
    }

    #[test]
    fn quantize_to_beat_rounds_to_nearest_beat() {
        let sr = 44100.0f64;
        let bpm = 120.0f64;
        let beat_dur = (60.0 / bpm) * sr;
        // 30% into beat 2 → rounds down to beat 2
        let pos = 2.0 * beat_dur + beat_dur * 0.3;
        let result = quantize_to_beat(pos, bpm, 0.0, sr);
        assert!((result - 2.0 * beat_dur).abs() < 1.0, "got {}", result);
    }

    #[test]
    fn quantize_to_beat_rounds_up_past_midpoint() {
        let sr = 44100.0f64;
        let bpm = 120.0f64;
        let beat_dur = (60.0 / bpm) * sr;
        // 70% into beat 2 → rounds up to beat 3
        let pos = 2.0 * beat_dur + beat_dur * 0.7;
        let result = quantize_to_beat(pos, bpm, 0.0, sr);
        assert!((result - 3.0 * beat_dur).abs() < 1.0, "got {}", result);
    }

    #[test]
    fn quantize_to_beat_never_returns_negative() {
        // Position very close to 0 should not produce a negative frame
        let result = quantize_to_beat(1.0, 120.0, 0.0, 44100.0);
        assert!(result >= 0.0, "got {}", result);
    }

    #[test]
    fn quantize_to_beat_respects_beat_offset() {
        let sr = 44100.0f64;
        let bpm = 120.0f64;
        let beat_dur = (60.0 / bpm) * sr;
        let offset = beat_dur * 0.25; // grid starts a quarter beat in
        // Position exactly on beat 1 with offset
        let pos = offset + beat_dur;
        let result = quantize_to_beat(pos, bpm, offset, sr);
        assert!((result - pos).abs() < 1.0, "got {}", result);
    }
}
