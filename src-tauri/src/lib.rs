mod audio;

use audio::{AppAudio, DeviceInfo, TrackInfo};
use std::sync::Arc;

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
    state
        .audio
        .deck(deck)
        .ok_or_else(|| format!("unknown deck: {}", deck))
}

// ── Commands ──────────────────────────────────────────────────────────────────

#[tauri::command]
async fn load_track(
    state: tauri::State<'_, AppState>,
    deck: String,
    path: String,
    analyze: bool,
) -> Result<TrackInfo, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let device_sample_rate = state.audio.device_sample_rate;

    // Run all CPU-heavy work in a single blocking thread.
    let (resampled, channels, bpm, silence_end, native_sr) =
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

            Ok((resampled, channels, bpm, silence_end, native_sr))
        })
        .await
        .map_err(|e| e.to_string())??;

    let total_frames = resampled.len() / channels;
    let duration = total_frames as f64 / device_sample_rate as f64;
    let silence_pos = silence_end * device_sample_rate as f64;

    log::info!(
        "load_track [{}]: analyze={} native_sr={} device_sr={} channels={} duration={:.2}s bpm={:?} silence_end={:.3}s silence_pos={:.0} frames",
        deck, analyze, native_sr, device_sample_rate, channels, duration, bpm, silence_end, silence_pos
    );

    {
        let mut deck = deck_arc.lock().unwrap();
        deck.samples = std::sync::Arc::new(resampled);
        deck.channels = channels;
        deck.device_sample_rate = device_sample_rate;
        deck.total_frames = total_frames;
        deck.duration = duration;
        deck.is_playing = false;
        deck.main_pos = silence_pos;
        deck.cue_pos = silence_pos;
        deck.loop_active = false;
        deck.loop_start = 0.0;
        deck.loop_end = 0.0;
        deck.playback_rate = 1.0;
        deck.nudge_factor = 1.0;
    }

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
    if d.is_playing {
        return Ok(());
    }
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
    get_deck(&state, &deck)?.lock().unwrap().gain = gain.clamp(0.0, 1.0);
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
    get_deck(&state, &deck)?
        .lock()
        .unwrap()
        .set_eq_band(&band, db);
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
    get_deck(&state, &deck)?.lock().unwrap().cue_active = active;
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
) -> Result<(), String> {
    state.audio.set_cue_device(&device_id)
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

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let audio = AppAudio::new().expect("failed to initialize audio engine");
    let app_state = AppState {
        audio: Arc::new(audio),
    };

    tauri::Builder::default()
        .manage(app_state)
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
            get_position,
            get_waveform_region,
            set_reloop,
            set_cue_active,
            list_audio_devices,
            set_cue_device,
            open_file_dialog,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
