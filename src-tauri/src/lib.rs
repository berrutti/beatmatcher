mod audio;

use audio::{AppAudio, ChannelStrip, DeviceInfo, TrackInfo};
use std::sync::Arc;
use tauri::Emitter;

pub struct AppState {
    pub audio: Arc<AppAudio>,
}

unsafe impl Send for AppState {}
unsafe impl Sync for AppState {}

// ── Helper ────────────────────────────────────────────────────────────────────

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

// ── Commands ──────────────────────────────────────────────────────────────────

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

    // Run all CPU-heavy work in a single blocking thread.
    let (samples, channels, bpm, silence_end, native_sr) =
        tokio::task::spawn_blocking(move || -> Result<_, String> {
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
                    audio::detect_bpm(mono, native_sr),
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

            Ok((std::sync::Arc::new(resampled), channels, bpm, silence_end, native_sr))
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

        let bass_scale = {
            let m = bass_band.iter().map(|&x| x.abs()).fold(0.0f32, f32::max);
            if m > 0.0 { 1.0 / m } else { 1.0 }
        };
        let mid_scale = {
            let m = mid_band.iter().map(|&x| x.abs()).fold(0.0f32, f32::max);
            if m > 0.0 { 1.0 / m } else { 1.0 }
        };
        let high_scale = {
            let m = high_band.iter().map(|&x| x.abs()).fold(0.0f32, f32::max);
            if m > 0.0 { 1.0 / m } else { 1.0 }
        };

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
        let pos = (sec * d.device_sample_rate as f64)
            .max(0.0)
            .min(d.total_frames as f64);
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
    let pos = (sec * d.device_sample_rate as f64)
        .max(0.0)
        .min(d.total_frames as f64);
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

#[tauri::command]
fn set_volume(
    state: tauri::State<'_, AppState>,
    deck: String,
    gain: f32,
) -> Result<(), String> {
    get_strip(&state, &deck)?.lock().unwrap().gain = gain.clamp(0.0, 1.0);
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

// Returns flat [bass_norm, mid_norm, high_norm, amplitude] * num_points.
// Each band value is normalized by its per-band global max so colors reflect
// spectral balance rather than absolute amplitude.
#[tauri::command]
fn get_spectral_waveform_region(
    state: tauri::State<'_, AppState>,
    deck: String,
    start_sec: f64,
    end_sec: f64,
    num_points: usize,
) -> Result<Vec<f32>, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let (samples, channels, bass, mid, high, bass_scale, mid_scale, high_scale, device_sr) = {
        let d = deck_arc.lock().unwrap();
        (
            std::sync::Arc::clone(&d.samples),
            d.channels,
            std::sync::Arc::clone(&d.bass_band),
            std::sync::Arc::clone(&d.mid_band),
            std::sync::Arc::clone(&d.high_band),
            d.bass_scale,
            d.mid_scale,
            d.high_scale,
            d.device_sample_rate,
        )
    };
    Ok(audio::compute_spectral_waveform_region(
        &samples, channels, &bass, &mid, &high,
        device_sr, bass_scale, mid_scale, high_scale,
        start_sec, end_sec, num_points,
    ))
}

#[tauri::command]
fn set_reloop(state: tauri::State<'_, AppState>, deck: String) -> Result<(), String> {
    let deck_arc = get_deck(&state, &deck)?;
    let mut d = deck_arc.lock().unwrap();
    if d.loop_end <= d.loop_start {
        return Ok(());
    }
    let start = d.loop_start;
    if d.is_playing {
        d.main_pos = start;
        d.cue_pos = start;
        d.loop_active = true;
    } else {
        d.main_pos = start;
        d.cue_pos = start;
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
fn files_info(paths: Vec<String>) -> Vec<Option<u64>> {
    paths
        .into_iter()
        .map(|p| std::fs::metadata(&p).ok().map(|m| m.len()))
        .collect()
}

#[tauri::command]
async fn analyze_track(path: String) -> Result<TrackInfo, String> {
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

        let bpm = audio::detect_bpm(mono, native_sr);
        let silence_end = audio::detect_silence_end(mono, native_sr);

        Ok(TrackInfo { duration, sample_rate: native_sr, bpm, silence_end })
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let audio = AppAudio::new().expect("failed to initialize audio engine");
    let app_state = AppState {
        audio: Arc::new(audio),
    };

    tauri::Builder::default()
        .manage(app_state)
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .build(),
            )?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_track,
            play,
            stop,
            seek,
            set_loop_region,
            set_loop_active,
            set_volume,
            set_playback_rate,
            set_nudge,
            set_eq,
            set_filter,
            get_position,
            get_waveform_region,
            get_spectral_waveform_region,
            set_reloop,
            set_cue_active,
            list_audio_devices,
            set_cue_device,
            set_main_device,
            open_file_dialog,
            files_info,
            analyze_track,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
