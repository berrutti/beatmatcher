use crate::audio::{self, ChannelStrip, CuePressOutcome, DeviceInfo, TrackInfo};
use crate::AppState;
use std::sync::Arc;
use tauri::Emitter;

pub(crate) fn get_deck(
    state: &tauri::State<'_, AppState>,
    deck: &str,
) -> Result<Arc<std::sync::Mutex<audio::DeckState>>, String> {
    state
        .audio
        .deck(deck)
        .ok_or_else(|| format!("unknown deck: {}", deck))
}

pub(crate) fn get_strip(
    state: &tauri::State<'_, AppState>,
    deck: &str,
) -> Result<Arc<std::sync::Mutex<ChannelStrip>>, String> {
    state
        .audio
        .strip(deck)
        .ok_or_else(|| format!("unknown deck: {}", deck))
}

fn sec_to_frame(sec: f64, sample_rate: u32, total_frames: usize) -> f64 {
    (sec * sample_rate as f64).clamp(0.0, total_frames as f64)
}

fn band_normalization_scale(band: &[f32]) -> f32 {
    let max = band.iter().map(|&x| x.abs()).fold(0.0f32, f32::max);
    if max > 0.0 {
        1.0 / max
    } else {
        1.0
    }
}

// Returned by all transport commands so the frontend can mirror deck state
// without any branching logic.
//
// loop_active vs loop_region_cleared are independent and mean different things:
//
//   loop_active         whether the loop is currently armed (playback loops).
//                       Can be false while a region is still defined, e.g. after
//                       seeking outside the region or calling exitLoop. The region
//                       persists so reloop can re-enter it.
//
//   loop_region_cleared the region itself was destroyed and the frontend should
//                       discard its cached loopRegion entirely (waveform overlay
//                       disappears). Only true when the cue point moves to a new
//                       position (CueMoved) or loop_in is pressed, because those
//                       actions invalidate the old loop_end.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeckSyncPayload {
    is_playing: bool,
    is_cueing: bool,
    cue_point_sec: f64,
    position_sec: f64,
    loop_active: bool,
    loop_region_cleared: bool,
}

impl DeckSyncPayload {
    pub(crate) fn from_deck(deck_state: &audio::DeckState, loop_region_cleared: bool) -> Self {
        let sr = deck_state.device_sample_rate as f64;
        Self {
            is_playing: deck_state.is_playing,
            is_cueing: deck_state.is_cueing,
            cue_point_sec: if sr > 0.0 {
                deck_state.cue_point / sr
            } else {
                0.0
            },
            position_sec: deck_state.position_sec(),
            loop_active: deck_state.loop_active,
            loop_region_cleared,
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoopOutResult {
    start_sec: f64,
    end_sec: f64,
    beats: i64,
    // Some when a late quantized press caused an immediate seek; frontend must sync positionCache.
    seek_to_sec: Option<f64>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NudgeResult {
    position_sec: f64,
    effective_rate: f64,
}

pub(crate) fn quantize_to_beat(pos_frames: f64, bpm: f64, beat_offset_frames: f64, sr: f64) -> f64 {
    let beat_dur = (60.0 / bpm) * sr;
    let index = ((pos_frames - beat_offset_frames) / beat_dur).round();
    (beat_offset_frames + index * beat_dur).max(0.0)
}

pub(crate) fn loop_in_core(deck_state: &mut audio::DeckState) -> Result<f64, String> {
    let sr = deck_state.device_sample_rate as f64;
    let bpm = deck_state.bpm.ok_or("no beat grid set")?;
    let in_frames = if deck_state.quantize {
        quantize_to_beat(deck_state.main_pos, bpm, deck_state.beat_offset_frames, sr)
    } else {
        deck_state.main_pos
    };
    deck_state.cue_point = in_frames;
    deck_state.loop_active = false;
    deck_state.loop_end = 0.0;
    Ok(in_frames / sr)
}

pub(crate) fn loop_out_core(
    deck_state: &mut audio::DeckState,
) -> Result<Option<LoopOutResult>, String> {
    let sr = deck_state.device_sample_rate as f64;
    let bpm = deck_state.bpm.ok_or("no beat grid set")?;
    let out_frames = if deck_state.quantize {
        quantize_to_beat(deck_state.main_pos, bpm, deck_state.beat_offset_frames, sr)
    } else {
        deck_state.main_pos
    };
    let in_frames = deck_state.cue_point;
    if out_frames <= in_frames {
        return Ok(None);
    }
    deck_state.loop_end = out_frames;
    deck_state.loop_active = true;
    // When quantized and pressed late, main_pos has already passed loop_end.
    // Immediately seek to cue_point + overshoot so the next audio callback
    // reads from the compensated position rather than the overshoot.
    let seek_to_sec = if deck_state.quantize && deck_state.main_pos > out_frames {
        let dur = out_frames - in_frames;
        let overshoot = deck_state.main_pos - out_frames;
        let new_pos = in_frames + overshoot % dur;
        deck_state.main_pos = new_pos;
        Some(new_pos / sr)
    } else {
        None
    };
    let start_sec = in_frames / sr;
    let end_sec = out_frames / sr;
    let beats = ((end_sec - start_sec) * bpm / 60.0).round() as i64;
    Ok(Some(LoopOutResult {
        start_sec,
        end_sec,
        beats,
        seek_to_sec,
    }))
}

#[tauri::command]
pub(crate) async fn load_track(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    deck: String,
    path: String,
    analyze: bool,
    beat_offset_sec: f64,
) -> Result<TrackInfo, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let device_sample_rate = state.audio.device_sample_rate;
    let bpm_min = state
        .audio
        .bpm_min
        .load(std::sync::atomic::Ordering::Relaxed) as f64;
    let bpm_max = state
        .audio
        .bpm_max
        .load(std::sync::atomic::Ordering::Relaxed) as f64;

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

            Ok((
                Arc::new(resampled),
                channels,
                bpm,
                silence_end,
                native_sr,
                cover_art,
            ))
        })
        .await
        .map_err(|e| e.to_string())??;

    let total_frames = samples.len() / channels;
    let duration = total_frames as f64 / device_sample_rate as f64;
    // beat_offset_sec is the user-calibrated position of beat 1 (saved per track, defaults to
    // silence_end). Setting main_pos, cue_pos, and cue_point all to the same value guarantees
    // that press_cue finds main_pos == cue_point and starts preview immediately. Previously only
    // main_pos was set here and cue_point was updated by a separate seek command, leaving a
    // window where they diverged and press_cue returned CueMoved instead of PreviewStarted.
    let start_pos = (beat_offset_sec * device_sample_rate as f64).clamp(0.0, total_frames as f64);

    log::info!(
        "load_track [{}]: analyze={} native_sr={} device_sr={} channels={} duration={:.2}s bpm={:?} silence_end={:.3}s beat_offset={:.3}s start_pos={:.0} frames",
        deck, analyze, native_sr, device_sample_rate, channels, duration, bpm, silence_end, beat_offset_sec, start_pos
    );

    {
        let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
        deck_state.samples = Arc::clone(&samples);
        deck_state.channels = channels;
        deck_state.device_sample_rate = device_sample_rate;
        deck_state.total_frames = total_frames;
        deck_state.duration = duration;
        deck_state.loaded_path = Some(path_for_log.clone());
        deck_state.is_playing = false;
        deck_state.is_cueing = false;
        deck_state.main_pos = start_pos;
        deck_state.cue_pos = start_pos;
        deck_state.cue_point = start_pos;
        deck_state.loop_active = false;
        deck_state.loop_end = 0.0;
        deck_state.bpm = None;
        deck_state.beat_offset_frames = 0.0;
        deck_state.playback_rate = 1.0;
        deck_state.nudge_factor = 1.0;
        deck_state.bass_band = Arc::new(Vec::new());
        deck_state.mid_band = Arc::new(Vec::new());
        deck_state.high_band = Arc::new(Vec::new());
        deck_state.bass_scale = 1.0;
        deck_state.mid_scale = 1.0;
        deck_state.high_scale = 1.0;
    }

    // Compute spectral bands in background; emit "bands-ready" when done so the
    // frontend can fetch waveform data without blocking track load.
    let deck_id = deck.clone();
    tokio::spawn(async move {
        let (bass_band, mid_band, high_band) = tokio::task::spawn_blocking(move || {
            audio::compute_spectral_bands(&samples, channels, device_sample_rate)
        })
        .await
        .unwrap_or_else(|_| (Vec::new(), Vec::new(), Vec::new()));

        let bass_scale = band_normalization_scale(&bass_band);
        let mid_scale = band_normalization_scale(&mid_band);
        let high_scale = band_normalization_scale(&high_band);

        {
            let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
            deck_state.bass_band = Arc::new(bass_band);
            deck_state.mid_band = Arc::new(mid_band);
            deck_state.high_band = Arc::new(high_band);
            deck_state.bass_scale = bass_scale;
            deck_state.mid_scale = mid_scale;
            deck_state.high_scale = high_scale;
        }

        app.emit("bands-ready", deck_id).ok();
    });

    state.log(
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
pub(crate) fn play(
    state: tauri::State<'_, AppState>,
    deck: String,
    from_sec: Option<f64>,
) -> Result<(), String> {
    let deck_arc = get_deck(&state, &deck)?;
    let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
    if let Some(sec) = from_sec {
        let pos = sec_to_frame(sec, deck_state.device_sample_rate, deck_state.total_frames);
        deck_state.main_pos = pos;
        deck_state.cue_pos = pos;
    } else {
        deck_state.cue_pos = deck_state.main_pos;
    }
    log::info!(
        "play [{}]: from_sec={:?} main_pos={:.0}",
        deck,
        from_sec,
        deck_state.main_pos
    );
    deck_state.is_playing = true;
    Ok(())
}

#[tauri::command]
pub(crate) fn stop(state: tauri::State<'_, AppState>, deck: String) -> Result<(), String> {
    get_deck(&state, &deck)?
        .lock()
        .expect("deck mutex poisoned")
        .is_playing = false;
    Ok(())
}

#[tauri::command]
pub(crate) fn press_cue(
    state: tauri::State<'_, AppState>,
    deck: String,
) -> Result<DeckSyncPayload, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let (outcome, payload) = {
        let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
        if deck_state.quantize {
            if let Some(bpm) = deck_state.bpm {
                let sr = deck_state.device_sample_rate as f64;
                deck_state.main_pos =
                    quantize_to_beat(deck_state.main_pos, bpm, deck_state.beat_offset_frames, sr);
            }
        }
        let had_loop = deck_state.loop_end > 0.0;
        let out = deck_state.press_cue();
        let loop_cleared = matches!(out, CuePressOutcome::CueMoved { .. }) && had_loop;
        if loop_cleared {
            deck_state.loop_active = false;
            deck_state.loop_end = 0.0;
        }
        (out, DeckSyncPayload::from_deck(&deck_state, loop_cleared))
    };
    let (event, cue_sec) = match outcome {
        CuePressOutcome::NoTrack => return Ok(payload),
        CuePressOutcome::PreviewStarted => ("cue_preview_start", payload.cue_point_sec),
        CuePressOutcome::CueMoved { new_cue_point_sec } => ("cue_move", new_cue_point_sec),
        CuePressOutcome::StoppedAtCue { cue_point_sec } => ("stopped_at_cue", cue_point_sec),
    };
    state.log(
        event,
        serde_json::json!({ "deck": deck, "cue_point_sec": cue_sec }),
    );
    Ok(payload)
}

#[tauri::command]
pub(crate) fn release_cue(
    state: tauri::State<'_, AppState>,
    deck: String,
) -> Result<DeckSyncPayload, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let (was_cueing, payload) = {
        let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
        let was = deck_state.is_cueing;
        deck_state.release_cue();
        (was, DeckSyncPayload::from_deck(&deck_state, false))
    };
    if was_cueing {
        state.log(
            "cue_preview_end",
            serde_json::json!({ "deck": deck, "cue_point_sec": payload.cue_point_sec }),
        );
    }
    Ok(payload)
}

#[tauri::command]
pub(crate) fn toggle_play(
    state: tauri::State<'_, AppState>,
    deck: String,
) -> Result<DeckSyncPayload, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let payload = {
        let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
        deck_state.toggle_play();
        DeckSyncPayload::from_deck(&deck_state, false)
    };
    state.log(
        if payload.is_playing { "play" } else { "stop" },
        serde_json::json!({ "deck": deck }),
    );
    Ok(payload)
}

#[tauri::command]
pub(crate) fn set_cue_and_stop(
    state: tauri::State<'_, AppState>,
    deck: String,
) -> Result<DeckSyncPayload, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let (was_playing, payload) = {
        let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
        let was = deck_state.is_playing;
        deck_state.set_cue_and_stop();
        (was, DeckSyncPayload::from_deck(&deck_state, false))
    };
    if was_playing {
        state.log(
            "cue_set_and_stop",
            serde_json::json!({ "deck": deck, "cue_point_sec": payload.cue_point_sec }),
        );
    }
    Ok(payload)
}

#[tauri::command]
pub(crate) fn stop_at_cue(
    state: tauri::State<'_, AppState>,
    deck: String,
) -> Result<DeckSyncPayload, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let (was_playing, payload) = {
        let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
        let was = deck_state.is_playing;
        deck_state.stop_at_cue();
        (was, DeckSyncPayload::from_deck(&deck_state, false))
    };
    if was_playing {
        state.log(
            "stop_at_cue",
            serde_json::json!({ "deck": deck, "cue_point_sec": payload.cue_point_sec }),
        );
    }
    Ok(payload)
}

#[tauri::command]
pub(crate) fn eject_track(state: tauri::State<'_, AppState>, deck: String) -> Result<(), String> {
    get_deck(&state, &deck)?
        .lock()
        .expect("deck mutex poisoned")
        .eject();
    state.log("eject_track", serde_json::json!({ "deck": deck }));
    Ok(())
}

#[tauri::command]
pub(crate) fn seek(
    state: tauri::State<'_, AppState>,
    deck: String,
    sec: f64,
) -> Result<DeckSyncPayload, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let payload = {
        let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
        let pos = sec_to_frame(sec, deck_state.device_sample_rate, deck_state.total_frames);
        log::info!("seek [{}]: {:.3}s -> frame {:.0}", deck, sec, pos);
        deck_state.main_pos = pos;
        deck_state.cue_pos = pos;
        if deck_state.loop_active && (pos < deck_state.cue_point || pos >= deck_state.loop_end) {
            deck_state.loop_active = false;
        }
        DeckSyncPayload::from_deck(&deck_state, false)
    };
    state.log("seek", serde_json::json!({ "deck": deck, "sec": sec }));
    Ok(payload)
}

#[tauri::command]
pub(crate) fn set_loop_region(
    state: tauri::State<'_, AppState>,
    deck: String,
    start_sec: f64,
    end_sec: f64,
) -> Result<(), String> {
    let deck_arc = get_deck(&state, &deck)?;
    let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
    let sr = deck_state.device_sample_rate as f64;
    deck_state.cue_point = start_sec * sr;
    deck_state.loop_end = end_sec * sr;
    Ok(())
}

#[tauri::command]
pub(crate) fn set_loop_active(
    state: tauri::State<'_, AppState>,
    deck: String,
    active: bool,
) -> Result<DeckSyncPayload, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let payload = {
        let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
        deck_state.loop_active = active;
        DeckSyncPayload::from_deck(&deck_state, false)
    };
    if !active {
        state.log("exit_loop", serde_json::json!({ "deck": deck }));
    }
    Ok(payload)
}

#[tauri::command]
pub(crate) fn set_beat_grid(
    state: tauri::State<'_, AppState>,
    deck: String,
    bpm: f64,
    beat_offset_sec: f64,
) -> Result<(), String> {
    let deck_arc = get_deck(&state, &deck)?;
    {
        let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
        deck_state.bpm = Some(bpm);
        deck_state.beat_offset_frames = beat_offset_sec * deck_state.device_sample_rate as f64;
    }
    state.log(
        "set_beat_grid",
        serde_json::json!({ "deck": deck, "bpm": bpm, "beat_offset_sec": beat_offset_sec }),
    );
    Ok(())
}

#[tauri::command]
pub(crate) fn set_loop_in(
    state: tauri::State<'_, AppState>,
    deck: String,
) -> Result<DeckSyncPayload, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let (payload, cue_sec, quantize) = {
        let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
        let sec = loop_in_core(&mut deck_state)?;
        let payload = DeckSyncPayload::from_deck(&deck_state, true);
        (payload, sec, deck_state.quantize)
    };
    state.log(
        "loop_in",
        serde_json::json!({ "deck": deck, "cue_sec": cue_sec, "quantized": quantize }),
    );
    Ok(payload)
}

#[tauri::command]
pub(crate) fn set_loop_out(
    state: tauri::State<'_, AppState>,
    deck: String,
) -> Result<Option<LoopOutResult>, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let (result, quantize) = {
        let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
        let quantize = deck_state.quantize;
        (loop_out_core(&mut deck_state)?, quantize)
    };
    if let Some(r) = &result {
        state.log(
            "loop_out",
            serde_json::json!({
                "deck": deck,
                "start_sec": r.start_sec,
                "end_sec": r.end_sec,
                "beats": r.beats,
                "quantized": quantize,
            }),
        );
    }
    Ok(result)
}

#[tauri::command]
pub(crate) fn clear_loop_region(
    state: tauri::State<'_, AppState>,
    deck: String,
) -> Result<(), String> {
    let deck_arc = get_deck(&state, &deck)?;
    let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
    deck_state.loop_active = false;
    deck_state.loop_end = 0.0;
    Ok(())
}

#[tauri::command]
pub(crate) fn set_volume(
    state: tauri::State<'_, AppState>,
    deck: String,
    gain: f32,
) -> Result<(), String> {
    get_strip(&state, &deck)?
        .lock()
        .expect("strip mutex poisoned")
        .set_gain(gain);
    state.log(
        "set_volume",
        serde_json::json!({ "deck": deck, "gain": gain }),
    );
    Ok(())
}

#[tauri::command]
pub(crate) fn set_playback_rate(
    state: tauri::State<'_, AppState>,
    deck: String,
    rate: f64,
) -> Result<(), String> {
    get_deck(&state, &deck)?
        .lock()
        .expect("deck mutex poisoned")
        .playback_rate = rate.max(0.1);
    state.log(
        "set_playback_rate",
        serde_json::json!({ "deck": deck, "rate": rate }),
    );
    Ok(())
}

#[tauri::command]
pub(crate) fn set_nudge(
    state: tauri::State<'_, AppState>,
    deck: String,
    percent: f64,
) -> Result<NudgeResult, String> {
    let result = {
        let deck_arc = get_deck(&state, &deck)?;
        let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
        deck_state.nudge_factor = 1.0 + percent / 100.0;
        NudgeResult {
            position_sec: deck_state.position_sec(),
            effective_rate: deck_state.playback_rate * deck_state.nudge_factor,
        }
    };
    state.log(
        "set_nudge",
        serde_json::json!({ "deck": deck, "percent": percent }),
    );
    Ok(result)
}

#[tauri::command]
pub(crate) fn set_quantize(
    state: tauri::State<'_, AppState>,
    deck: String,
    quantize: bool,
) -> Result<(), String> {
    get_deck(&state, &deck)?
        .lock()
        .expect("deck mutex poisoned")
        .quantize = quantize;
    Ok(())
}

#[tauri::command]
pub(crate) fn set_eq(
    state: tauri::State<'_, AppState>,
    deck: String,
    band: String,
    db: f32,
) -> Result<(), String> {
    get_strip(&state, &deck)?
        .lock()
        .expect("strip mutex poisoned")
        .set_eq_band(&band, db);
    state.log(
        "set_eq",
        serde_json::json!({ "deck": deck, "band": band, "db": db }),
    );
    Ok(())
}

#[tauri::command]
pub(crate) fn set_filter(
    state: tauri::State<'_, AppState>,
    deck: String,
    value: f32,
) -> Result<(), String> {
    get_strip(&state, &deck)?
        .lock()
        .expect("strip mutex poisoned")
        .set_filter(value);
    state.log(
        "set_filter",
        serde_json::json!({ "deck": deck, "value": value }),
    );
    Ok(())
}

#[tauri::command]
pub(crate) fn set_filter_active(
    state: tauri::State<'_, AppState>,
    deck: String,
    active: bool,
) -> Result<(), String> {
    get_strip(&state, &deck)?
        .lock()
        .expect("strip mutex poisoned")
        .set_filter_active(active);
    state.log(
        "set_filter_active",
        serde_json::json!({ "deck": deck, "active": active }),
    );
    Ok(())
}

// Returns flat [bass_norm, mid_norm, high_norm, amplitude] * num_points as raw
// f32 little-endian bytes. Binary transfer avoids JSON serialization overhead
// that would otherwise cause GC pauses on large waveform loads.
#[tauri::command]
pub(crate) async fn get_spectral_waveform_region(
    state: tauri::State<'_, AppState>,
    deck: String,
    start_sec: f64,
    end_sec: f64,
    num_points: usize,
) -> Result<tauri::ipc::Response, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let (samples, channels, bass, mid, high, bass_scale, mid_scale, high_scale, device_sr) = {
        let deck_state = deck_arc.lock().expect("deck mutex poisoned");
        (
            Arc::clone(&deck_state.samples),
            deck_state.channels,
            Arc::clone(&deck_state.bass_band),
            Arc::clone(&deck_state.mid_band),
            Arc::clone(&deck_state.high_band),
            deck_state.bass_scale,
            deck_state.mid_scale,
            deck_state.high_scale,
            deck_state.device_sample_rate,
        )
    };
    let floats = tokio::task::spawn_blocking(move || {
        audio::compute_spectral_waveform_region(
            &samples, channels, &bass, &mid, &high, device_sr, bass_scale, mid_scale, high_scale,
            start_sec, end_sec, num_points,
        )
    })
    .await
    .map_err(|e| e.to_string())?;
    // Reinterpret the f32 slice as bytes without an extra allocation.
    let bytes = unsafe {
        std::slice::from_raw_parts(floats.as_ptr() as *const u8, floats.len() * 4).to_vec()
    };
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub(crate) fn set_reloop(
    state: tauri::State<'_, AppState>,
    deck: String,
) -> Result<DeckSyncPayload, String> {
    let deck_arc = get_deck(&state, &deck)?;
    let payload = {
        let mut deck_state = deck_arc.lock().expect("deck mutex poisoned");
        if deck_state.loop_end > deck_state.cue_point {
            deck_state.main_pos = deck_state.cue_point;
            deck_state.cue_pos = deck_state.cue_point;
            if deck_state.is_playing {
                deck_state.loop_active = true;
            }
        }
        DeckSyncPayload::from_deck(&deck_state, false)
    };
    state.log("reloop", serde_json::json!({ "deck": deck }));
    Ok(payload)
}

#[tauri::command]
pub(crate) fn set_cue_active(
    state: tauri::State<'_, AppState>,
    deck: String,
    active: bool,
) -> Result<(), String> {
    get_strip(&state, &deck)?
        .lock()
        .expect("strip mutex poisoned")
        .cue_active = active;
    state.log(
        "set_cue_active",
        serde_json::json!({ "deck": deck, "active": active }),
    );
    Ok(())
}

#[tauri::command]
pub(crate) fn list_audio_devices(state: tauri::State<'_, AppState>) -> Vec<DeviceInfo> {
    state.audio.list_devices()
}

#[tauri::command]
pub(crate) fn set_cue_device(
    state: tauri::State<'_, AppState>,
    device_id: String,
    channel_offset: usize,
) -> Result<(), String> {
    state.audio.set_cue_device(&device_id, channel_offset)
}

#[tauri::command]
pub(crate) fn set_main_device(
    state: tauri::State<'_, AppState>,
    device_id: String,
    channel_offset: usize,
) -> Result<(), String> {
    state.audio.set_main_device(&device_id, channel_offset)
}

#[tauri::command]
pub(crate) async fn open_file_dialog() -> Option<String> {
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
pub(crate) async fn pick_save_path(format: String) -> Option<String> {
    let (label, ext, name) = match format.as_str() {
        "flac" => ("FLAC Audio", "flac", "mix.flac"),
        "session" => ("Beatmatcher Session", "bms", "mix.bms"),
        _ => ("WAV Audio", "wav", "mix.wav"),
    };
    rfd::AsyncFileDialog::new()
        .add_filter(label, &[ext])
        .set_file_name(name)
        .save_file()
        .await
        .map(|f| f.path().to_string_lossy().into_owned())
}

#[tauri::command]
pub(crate) fn get_master_level(state: tauri::State<'_, AppState>) -> [f32; 2] {
    state.audio.get_master_level()
}

#[tauri::command]
pub(crate) fn get_deck_levels(
    state: tauri::State<'_, AppState>,
) -> std::collections::HashMap<String, [f32; 2]> {
    state.audio.get_deck_levels()
}

#[tauri::command]
pub(crate) fn start_recording(
    state: tauri::State<'_, AppState>,
    bit_depth: u16,
    use_flac: bool,
    record_session: bool,
) -> Result<(), String> {
    {
        let mut session = state.session.lock().expect("session mutex poisoned");
        *session = if record_session {
            Some(crate::SessionLogger::new())
        } else {
            None
        };
        if let Some(logger) = session.as_mut() {
            // 0 means "driver default"; macOS Core Audio default is 512 frames.
            let buf = state.audio.get_buffer_frames();
            let buffer_size_frames = if buf == 0 { 512 } else { buf };
            logger.log(
                "recording_start",
                serde_json::json!({
                    "buffer_size_frames": buffer_size_frames,
                }),
            );
            for deck_id in ["A", "B", "C", "D"] {
                let Some(arc) = state.audio.deck(deck_id) else {
                    continue;
                };
                let deck_state = arc.lock().expect("deck mutex poisoned");
                let Some(ref path) = deck_state.loaded_path else {
                    continue;
                };
                logger.log(
                    "deck_snapshot",
                    serde_json::json!({
                        "deck": deck_id,
                        "path": path,
                        "position_sec": deck_state.main_pos / deck_state.device_sample_rate as f64,
                        "cue_point_sec": deck_state.cue_point / deck_state.device_sample_rate as f64,
                        "is_playing": deck_state.is_playing,
                        "bpm": deck_state.bpm,
                        "playback_rate": deck_state.playback_rate,
                        "loop_active": deck_state.loop_active,
                        "loop_end_sec": deck_state.loop_end / deck_state.device_sample_rate as f64,
                    }),
                );
            }
        }
    }
    state.audio.start_recording(bit_depth, use_flac)
}

#[tauri::command]
pub(crate) async fn stop_recording(state: tauri::State<'_, AppState>) -> Result<String, String> {
    {
        let mut session = state.session.lock().expect("session mutex poisoned");
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
pub(crate) async fn read_track_tags(path: String) -> audio::TrackTags {
    tokio::task::spawn_blocking(move || audio::read_tags(&path))
        .await
        .unwrap_or(audio::TrackTags {
            title: None,
            artist: None,
        })
}

#[tauri::command]
pub(crate) fn save_recording(
    state: tauri::State<'_, AppState>,
    src: String,
    dest: String,
) -> Result<(), String> {
    if std::fs::rename(&src, &dest).is_err() {
        // rename fails across filesystems; fall back to copy then delete
        std::fs::copy(&src, &dest).map_err(|e| e.to_string())?;
        std::fs::remove_file(&src).ok();
    }
    if let Some(log) = state
        .session
        .lock()
        .expect("session mutex poisoned")
        .as_mut()
        .and_then(|l| l.take_pending())
    {
        let stem = dest
            .strip_suffix(".wav")
            .or_else(|| dest.strip_suffix(".WAV"))
            .or_else(|| dest.strip_suffix(".flac"))
            .or_else(|| dest.strip_suffix(".FLAC"))
            .unwrap_or(&dest);
        let log_dest = format!("{}.bms", stem);
        std::fs::write(&log_dest, log.as_bytes()).ok();
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn discard_recording(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    state
        .session
        .lock()
        .expect("session mutex poisoned")
        .as_mut()
        .and_then(|l| l.take_pending());
    std::fs::remove_file(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn save_bms_only(
    state: tauri::State<'_, AppState>,
    src: String,
    dest: String,
) -> Result<(), String> {
    std::fs::remove_file(&src).ok();
    if let Some(log) = state
        .session
        .lock()
        .expect("session mutex poisoned")
        .as_mut()
        .and_then(|l| l.take_pending())
    {
        std::fs::write(&dest, log.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn render_session_to_file(
    state: tauri::State<'_, AppState>,
    session_path: String,
    output_path: String,
    use_flac: bool,
) -> Result<(), String> {
    // Prefer the in-memory session so unsaved edits are rendered too.
    let cached = state
        .session_files
        .lock()
        .expect("session files mutex poisoned")
        .get(&session_path)
        .cloned();
    tokio::task::spawn_blocking(move || {
        let session = match cached {
            Some(cached_session) => cached_session,
            None => {
                let json = std::fs::read_to_string(&session_path)
                    .map_err(|e| format!("{session_path}: {e}"))?;
                std::sync::Arc::new(
                    serde_json::from_str::<crate::offline_render::SessionFile>(&json)
                        .map_err(|e| format!("parse error: {e}"))?,
                )
            }
        };
        let sample_rate = 44100u32;
        let rendered = crate::offline_render::render_session(&session, sample_rate, 0)?;
        if use_flac {
            crate::offline_render::write_flac_f32(&output_path, &rendered, sample_rate)
        } else {
            crate::offline_render::write_wav_f32(&output_path, &rendered, sample_rate, 2)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) fn save_session(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content.as_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn set_master_gain(state: tauri::State<'_, AppState>, gain: f32) {
    state.audio.monitor.set_master_gain(gain);
    state.log("set_master_gain", serde_json::json!({ "gain": gain }));
}

#[tauri::command]
pub(crate) fn set_cue_mix(state: tauri::State<'_, AppState>, mix: f32) {
    state.audio.monitor.set_cue_mix(mix);
}

#[tauri::command]
pub(crate) fn set_limiter_enabled(state: tauri::State<'_, AppState>, enabled: bool) {
    state.audio.monitor.set_limiter_enabled(enabled);
}

#[tauri::command]
pub(crate) fn set_buffer_size(
    state: tauri::State<'_, AppState>,
    frames: u32,
) -> Result<(), String> {
    state.audio.set_buffer_frames(frames)
}

#[tauri::command]
pub(crate) fn set_bpm_range(state: tauri::State<'_, AppState>, min: u32, max: u32) {
    state.audio.set_bpm_range(min, max);
}

#[tauri::command]
pub(crate) fn files_info(paths: Vec<String>) -> Vec<Option<u64>> {
    paths
        .into_iter()
        .map(|p| std::fs::metadata(&p).ok().map(|m| m.len()))
        .collect()
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

#[tauri::command]
pub(crate) fn scan_folder(path: String) -> Vec<String> {
    let mut results = Vec::new();
    scan_dir_recursive(std::path::Path::new(&path), &mut results);
    results
}

#[tauri::command]
pub(crate) async fn analyze_track(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<TrackInfo, String> {
    let bpm_min = state
        .audio
        .bpm_min
        .load(std::sync::atomic::Ordering::Relaxed) as f64;
    let bpm_max = state
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

#[tauri::command]
pub(crate) fn read_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrackAmplitudeWaveform {
    duration_sec: f64,
    amps: Vec<f32>,
}

#[tauri::command]
pub(crate) async fn get_track_amplitude_waveform(
    path: String,
    num_points: usize,
) -> Result<TrackAmplitudeWaveform, String> {
    tokio::task::spawn_blocking(move || {
        let (samples, channels, sample_rate) =
            audio::decode_audio(&path).map_err(|e| e.to_string())?;
        let total_frames = samples.len() / channels;
        let duration_sec = total_frames as f64 / sample_rate as f64;
        let amps = audio::compute_amplitude_waveform(&samples, channels, num_points);
        Ok(TrackAmplitudeWaveform { duration_sec, amps })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio::DeckState;

    const SR: u32 = 44100;
    const SR_F: f64 = SR as f64;
    const BPM: f64 = 120.0;

    fn beat_dur() -> f64 {
        (60.0 / BPM) * SR_F
    }

    fn deck_with_grid(duration_secs: f64) -> DeckState {
        let mut deck_state = DeckState::loaded_for_testing(SR, duration_secs);
        deck_state.bpm = Some(BPM);
        deck_state.beat_offset_frames = 0.0;
        deck_state
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
        let pos = beat_dur() * 4.0;
        let result = quantize_to_beat(pos, BPM, 0.0, SR_F);
        assert!((result - pos).abs() < 1.0, "got {}", result);
    }

    #[test]
    fn quantize_to_beat_rounds_to_nearest_beat() {
        let pos = beat_dur() * 2.3; // 30% into beat 2, closer to beat 2
        let result = quantize_to_beat(pos, BPM, 0.0, SR_F);
        let expected = beat_dur() * 2.0;
        assert!((result - expected).abs() < 1.0, "got {}", result);
    }

    #[test]
    fn quantize_to_beat_rounds_up_past_midpoint() {
        let pos = beat_dur() * 2.7; // 70% into beat 2, closer to beat 3
        let result = quantize_to_beat(pos, BPM, 0.0, SR_F);
        let expected = beat_dur() * 3.0;
        assert!((result - expected).abs() < 1.0, "got {}", result);
    }

    #[test]
    fn quantize_to_beat_never_returns_negative() {
        let result = quantize_to_beat(1.0, 120.0, 0.0, SR_F);
        assert!(result >= 0.0);
    }

    #[test]
    fn quantize_to_beat_respects_beat_offset() {
        let offset = beat_dur() * 0.25;
        let pos = offset + beat_dur();
        let result = quantize_to_beat(pos, BPM, offset, SR_F);
        assert!((result - pos).abs() < 1.0, "got {}", result);
    }

    // --- press_cue with quantize (command-layer logic) ---

    // Mirrors the quantize-then-press sequence in the press_cue Tauri command.
    fn press_cue_quantized(deck_state: &mut DeckState) -> CuePressOutcome {
        if let Some(bpm) = deck_state.bpm {
            let sr = deck_state.device_sample_rate as f64;
            deck_state.main_pos =
                quantize_to_beat(deck_state.main_pos, bpm, deck_state.beat_offset_frames, sr);
        }
        deck_state.press_cue()
    }

    #[test]
    fn press_cue_quantized_snaps_new_cue_to_nearest_beat() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.cue_point = 0.0;
        deck_state.main_pos = beat_dur() * 2.3; // closer to beat 2
        let outcome = press_cue_quantized(&mut deck_state);
        let expected = beat_dur() * 2.0;
        assert_eq!(
            outcome,
            CuePressOutcome::CueMoved {
                new_cue_point_sec: expected / SR_F
            }
        );
        assert!(
            (deck_state.cue_point - expected).abs() < 1.0,
            "cue_point must snap to nearest beat"
        );
        assert!(
            (deck_state.main_pos - expected).abs() < 1.0,
            "main_pos must also snap"
        );
    }

    #[test]
    fn press_cue_quantized_rounds_up_past_midpoint() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.cue_point = 0.0;
        deck_state.main_pos = beat_dur() * 2.7; // past midpoint → rounds to beat 3
        press_cue_quantized(&mut deck_state);
        let expected = beat_dur() * 3.0;
        assert!((deck_state.cue_point - expected).abs() < 1.0);
    }

    #[test]
    fn press_cue_quantized_moves_cue_clears_loop() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.cue_point = 0.0;
        deck_state.loop_end = beat_dur() * 4.0;
        deck_state.loop_active = true;
        deck_state.main_pos = beat_dur() * 2.0;
        // Simulate press_cue Tauri command: quantize then check loop_cleared
        let had_loop = deck_state.loop_end > 0.0;
        let out = press_cue_quantized(&mut deck_state);
        let loop_cleared = matches!(out, CuePressOutcome::CueMoved { .. }) && had_loop;
        if loop_cleared {
            deck_state.loop_active = false;
            deck_state.loop_end = 0.0;
        }
        assert!(loop_cleared, "moving the cue must clear the existing loop");
        assert!(!deck_state.loop_active);
        assert_eq!(deck_state.loop_end, 0.0);
    }

    #[test]
    fn press_cue_quantized_preview_at_cue_does_not_clear_loop() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.cue_point = beat_dur() * 2.0;
        deck_state.loop_end = beat_dur() * 4.0;
        deck_state.loop_active = true;
        deck_state.main_pos = deck_state.cue_point; // already at cue → preview, not move
        let had_loop = deck_state.loop_end > 0.0;
        let out = press_cue_quantized(&mut deck_state);
        let loop_cleared = matches!(out, CuePressOutcome::CueMoved { .. }) && had_loop;
        assert!(!loop_cleared, "preview at cue must not clear the loop");
        assert!(deck_state.loop_active);
    }

    // --- loop_in_core ---

    #[test]
    fn loop_in_returns_current_position_when_not_quantized() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.main_pos = beat_dur() * 2.7; // between beats
        let sec = loop_in_core(&mut deck_state).unwrap();
        assert!((sec - deck_state.main_pos / SR_F).abs() < 1e-9);
    }

    #[test]
    fn loop_in_snaps_to_beat_when_quantized() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = true;
        deck_state.main_pos = beat_dur() * 2.3; // 30% into beat 2 → should snap to beat 2
        let sec = loop_in_core(&mut deck_state).unwrap();
        let expected = 2.0 * beat_dur() / SR_F;
        assert!(
            (sec - expected).abs() < 1e-3,
            "expected ~{:.4} got {:.4}",
            expected,
            sec
        );
    }

    #[test]
    fn loop_in_updates_cue_point_to_playhead() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = 0.0;
        deck_state.main_pos = beat_dur() * 3.0;
        loop_in_core(&mut deck_state).unwrap();
        assert!(
            (deck_state.cue_point - beat_dur() * 3.0).abs() < 1e-9,
            "cue_point must move to playhead"
        );
    }

    #[test]
    fn loop_in_does_not_stop_playback() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.is_playing = true;
        deck_state.main_pos = beat_dur() * 3.0;
        loop_in_core(&mut deck_state).unwrap();
        assert!(deck_state.is_playing, "loop_in must not stop playback");
    }

    #[test]
    fn loop_in_clears_existing_loop_region() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = beat_dur();
        deck_state.loop_end = beat_dur() * 3.0;
        deck_state.loop_active = true;
        deck_state.main_pos = beat_dur() * 4.0;
        loop_in_core(&mut deck_state).unwrap();
        assert!(!deck_state.loop_active);
        assert!(
            (deck_state.cue_point - beat_dur() * 4.0).abs() < 1e-9,
            "cue_point updated to new loop-in position"
        );
        assert_eq!(deck_state.loop_end, 0.0);
    }

    #[test]
    fn loop_in_fails_without_beat_grid() {
        let mut deck_state = DeckState::loaded_for_testing(SR, 10.0);
        deck_state.quantize = true;
        let result = loop_in_core(&mut deck_state);
        assert!(result.is_err());
    }

    // --- loop_out_core ---

    #[test]
    fn loop_out_creates_region_using_cue_point_as_start() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = beat_dur() * 2.0;
        deck_state.main_pos = beat_dur() * 4.0;
        let result = loop_out_core(&mut deck_state).unwrap().unwrap();
        assert!((result.start_sec - beat_dur() * 2.0 / SR_F).abs() < 1e-6);
        assert!((result.end_sec - beat_dur() * 4.0 / SR_F).abs() < 1e-6);
        assert!(deck_state.loop_active);
    }

    #[test]
    fn loop_out_returns_none_when_cue_is_past_out() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = beat_dur() * 3.0;
        deck_state.main_pos = beat_dur() * 2.0; // playhead before cue → no loop
        let result = loop_out_core(&mut deck_state).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn loop_out_quantized_snaps_end_to_beat() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = true;
        deck_state.cue_point = 0.0;
        deck_state.main_pos = beat_dur() * 4.0 + beat_dur() * 0.1; // 10% past beat 4
        let result = loop_out_core(&mut deck_state).unwrap().unwrap();
        let expected_end = beat_dur() * 4.0 / SR_F;
        assert!((result.end_sec - expected_end).abs() < 1e-3);
    }

    #[test]
    fn loop_out_quantized_compensates_late_press_position() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = true;
        let out_beat = beat_dur() * 4.0;
        deck_state.cue_point = 0.0;
        // Position is 5% past the loop end beat → late press
        deck_state.main_pos = out_beat + beat_dur() * 0.05;
        let result = loop_out_core(&mut deck_state).unwrap().unwrap();
        assert!(
            result.seek_to_sec.is_some(),
            "expected seek compensation for late press"
        );
        let compensated = result.seek_to_sec.unwrap();
        assert!(compensated >= result.start_sec);
        assert!(compensated < result.end_sec);
        assert!((deck_state.main_pos - compensated * SR_F).abs() < 1.0);
    }

    #[test]
    fn loop_out_returns_none_when_cue_equals_out() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = 0.0;
        deck_state.main_pos = 0.0; // out == cue → zero-length, rejected
        let result = loop_out_core(&mut deck_state).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn loop_out_arms_only_when_playhead_is_strictly_after_cue() {
        let cue = beat_dur() * 2.0;

        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = cue;
        deck_state.main_pos = beat_dur() * 4.0;
        assert!(
            loop_out_core(&mut deck_state).unwrap().is_some(),
            "playhead after cue: loop must arm"
        );
        assert!(deck_state.loop_active);

        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = cue;
        deck_state.main_pos = cue;
        assert!(
            loop_out_core(&mut deck_state).unwrap().is_none(),
            "playhead at cue: must not arm"
        );
        assert!(!deck_state.loop_active);

        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = cue;
        deck_state.main_pos = beat_dur() * 1.0;
        assert!(
            loop_out_core(&mut deck_state).unwrap().is_none(),
            "playhead before cue: must not arm"
        );
        assert!(!deck_state.loop_active);
    }

    #[test]
    fn loop_out_beat_count_matches_region_duration() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = 0.0;
        deck_state.main_pos = beat_dur() * 4.0;
        let result = loop_out_core(&mut deck_state).unwrap().unwrap();
        assert_eq!(result.beats, 4);
    }

    // --- seek: loop-arm rules ---
    //
    // Mirrors the seek command logic (pos update + conditional loop disarm).

    fn simulate_seek(deck_state: &mut DeckState, pos: f64) {
        deck_state.main_pos = pos;
        deck_state.cue_pos = pos;
        if deck_state.loop_active && (pos < deck_state.cue_point || pos >= deck_state.loop_end) {
            deck_state.loop_active = false;
        }
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

    // --- load_track initialization (start_pos invariant) ---

    // Simulate the deck state that load_track produces for a given beat_offset_sec.
    fn load_deck_at_beat_offset(beat_offset_sec: f64, duration_secs: f64) -> DeckState {
        let mut d = DeckState::loaded_for_testing(SR, duration_secs);
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
        // Regression: before start_pos was introduced, seek(beatOffset) moved main_pos but
        // left cue_point at silence_pos. This caused CueMoved on the first press.
        let mut d = load_deck_at_beat_offset(0.0, 10.0); // silence_pos = 0
        d.main_pos = 1.5 * SR_F; // beatOffset != silence_pos
                                 // cue_point is still 0. the old broken state
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
        // Tracks with a non-zero beat offset (user-adjusted grid) must also work on first press.
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
