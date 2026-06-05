// Offline session renderer and comparison tool.
//
// Replays a .session.json file through the audio engine without a real audio
// device, producing a rendered WAV. When compared against the WAV that was
// recorded during the original live session, any divergence indicates a bug in
// the event recording or replay logic.
//
// Usage (binary):
//   cargo run --bin compare_session -- <session.json> <recorded.wav> [output.wav]
//
// The binary prints per-channel RMS difference (dBFS), max absolute sample
// error, and the frame index of the first divergence above 1e-4.

use crate::audio::{self, ChannelStrip, DeckState, LimiterState};
use std::collections::HashMap;
use std::sync::Arc;

// ── WAV I/O ─────────────────────────────────────────────────────────────────

pub fn read_wav_f32(path: &str) -> Result<(Vec<f32>, u32, u16), String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut f = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut b4 = [0u8; 4];

    macro_rules! r4 {
        () => {{
            f.read_exact(&mut b4).map_err(|e| e.to_string())?;
            b4
        }};
    }
    macro_rules! ru32 {
        () => {
            u32::from_le_bytes(r4!())
        };
    }
    macro_rules! ru16 {
        () => {{
            let mut b = [0u8; 2];
            f.read_exact(&mut b).map_err(|e| e.to_string())?;
            u16::from_le_bytes(b)
        }};
    }

    if &r4!() != b"RIFF" {
        return Err(format!("{path}: not a RIFF file"));
    }
    let _ = ru32!(); // file size
    if &r4!() != b"WAVE" {
        return Err(format!("{path}: not a WAVE file"));
    }

    let mut sample_rate = 0u32;
    let mut bit_depth = 0u16;
    let mut channels = 0u16;
    let mut data_offset = 0u64;
    let mut data_bytes = 0u32;

    loop {
        let mut id = [0u8; 4];
        if f.read_exact(&mut id).is_err() {
            break;
        }
        let chunk_size = ru32!();
        match &id {
            b"fmt " => {
                let _audio_fmt = ru16!();
                channels = ru16!();
                sample_rate = ru32!();
                let _ = ru32!(); // byte rate
                let _ = ru16!(); // block align
                bit_depth = ru16!();
                let extra = chunk_size.saturating_sub(16);
                if extra > 0 {
                    f.seek(SeekFrom::Current(extra as i64)).ok();
                }
            }
            b"data" => {
                data_bytes = chunk_size;
                data_offset = f.stream_position().map_err(|e| e.to_string())?;
                break;
            }
            _ => {
                f.seek(SeekFrom::Current(chunk_size as i64)).ok();
            }
        }
    }

    if data_offset == 0 {
        return Err(format!("{path}: no data chunk"));
    }

    let mut raw = vec![0u8; data_bytes as usize];
    f.seek(SeekFrom::Start(data_offset))
        .map_err(|e| e.to_string())?;
    f.read_exact(&mut raw).map_err(|e| e.to_string())?;

    let samples: Vec<f32> = match bit_depth {
        16 => raw
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
            .collect(),
        24 => raw
            .chunks_exact(3)
            .map(|b| {
                let v =
                    i32::from_le_bytes([b[0], b[1], b[2], if b[2] & 0x80 != 0 { 0xff } else { 0 }]);
                v as f32 / 8_388_608.0
            })
            .collect(),
        32 => raw
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect(),
        _ => return Err(format!("{path}: unsupported bit depth {bit_depth}")),
    };

    Ok((samples, sample_rate, channels))
}

pub fn write_wav_f32(
    path: &str,
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
) -> Result<(), String> {
    use std::io::Write;
    let mut f = std::fs::File::create(path).map_err(|e| format!("{path}: {e}"))?;
    let byte_count = (samples.len() * 4) as u32;
    f.write_all(b"RIFF").map_err(|e| e.to_string())?;
    f.write_all(&(36 + byte_count).to_le_bytes())
        .map_err(|e| e.to_string())?;
    f.write_all(b"WAVE").map_err(|e| e.to_string())?;
    f.write_all(b"fmt ").map_err(|e| e.to_string())?;
    f.write_all(&16u32.to_le_bytes())
        .map_err(|e| e.to_string())?;
    f.write_all(&3u16.to_le_bytes())
        .map_err(|e| e.to_string())?; // IEEE float
    f.write_all(&channels.to_le_bytes())
        .map_err(|e| e.to_string())?;
    f.write_all(&sample_rate.to_le_bytes())
        .map_err(|e| e.to_string())?;
    f.write_all(&(sample_rate * channels as u32 * 4).to_le_bytes())
        .map_err(|e| e.to_string())?;
    f.write_all(&(channels * 4).to_le_bytes())
        .map_err(|e| e.to_string())?;
    f.write_all(&32u16.to_le_bytes())
        .map_err(|e| e.to_string())?;
    f.write_all(b"data").map_err(|e| e.to_string())?;
    f.write_all(&byte_count.to_le_bytes())
        .map_err(|e| e.to_string())?;
    for &s in samples {
        f.write_all(&s.to_le_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ── Session JSON ─────────────────────────────────────────────────────────────

#[derive(Debug, Default, serde::Deserialize)]
pub struct SessionEvent {
    pub elapsed_ms: f64,
    #[serde(rename = "type")]
    pub event_type: String,
    pub deck: Option<String>,
    pub path: Option<String>,
    pub sec: Option<f64>,
    pub gain: Option<f32>,
    pub band: Option<String>,
    pub db: Option<f32>,
    pub value: Option<f32>,
    pub active: Option<bool>,
    pub rate: Option<f64>,
    pub percent: Option<f64>,
    pub beat_offset_sec: Option<f64>,
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    pub is_playing: Option<bool>,
    pub position_sec: Option<f64>,
    pub cue_point_sec: Option<f64>,
    pub loop_active: Option<bool>,
    pub loop_end_sec: Option<f64>,
    pub bpm: Option<f64>,
    pub playback_rate: Option<f64>,
    pub cue_sec: Option<f64>,
    pub duration: Option<f64>,
    pub buffer_size_frames: Option<u32>,
}

#[derive(Debug, serde::Deserialize)]
pub struct SessionFile {
    pub events: Vec<SessionEvent>,
}

// ── Comparison result ────────────────────────────────────────────────────────

pub struct CompareResult {
    pub total_frames: usize,
    pub compared_frames: usize,
    pub max_abs_diff: f32,
    pub rms_diff_db: f32,
    pub first_divergence_frame: Option<usize>,
    pub sample_rate: u32,
}

// ── Entry point ──────────────────────────────────────────────────────────────

pub fn render_and_compare(
    session_path: &str,
    reference_path: &str,
    output_path: Option<&str>,
) -> Result<CompareResult, String> {
    let (reference, sample_rate, ref_channels) = read_wav_f32(reference_path)?;
    if ref_channels != 2 {
        return Err(format!(
            "reference WAV must be stereo (got {ref_channels} channels)"
        ));
    }

    let json = std::fs::read_to_string(session_path).map_err(|e| format!("{session_path}: {e}"))?;
    let session: SessionFile =
        serde_json::from_str(&json).map_err(|e| format!("parse error: {e}"))?;

    let rendered = render_session(&session, sample_rate, reference.len())?;

    if let Some(out) = output_path {
        write_wav_f32(out, &rendered, sample_rate, 2)?;
    }

    let mut result = diff_signals(&reference, &rendered);
    result.sample_rate = sample_rate;
    Ok(result)
}

fn diff_signals(a: &[f32], b: &[f32]) -> CompareResult {
    let len = a.len().min(b.len());
    let total_frames = len / 2;
    let mut sum_sq = 0.0f64;
    let mut max_diff = 0.0f32;
    let mut first_div = None;
    const THRESH: f32 = 1e-4;

    for i in 0..len {
        let d = (a[i] - b[i]).abs();
        if d > max_diff {
            max_diff = d;
        }
        sum_sq += (d as f64).powi(2);
        if first_div.is_none() && d > THRESH {
            first_div = Some(i / 2);
        }
    }

    let rms = (sum_sq / len as f64).sqrt() as f32;
    let rms_db = if rms > 0.0 {
        20.0 * rms.log10()
    } else {
        f32::NEG_INFINITY
    };

    if let Some(div_frame) = first_div {
        let start = (div_frame * 2).saturating_sub(4);
        let end = (start + 40).min(len);
        eprintln!("\n--- sample dump around divergence (frame {div_frame}) ---");
        eprintln!(
            "{:>8}  {:>12}  {:>12}  {:>12}",
            "frame", "reference", "rendered", "|diff|"
        );
        for i in (start..end).step_by(2) {
            let frame = i / 2;
            let rl = a.get(i).copied().unwrap_or(0.0);
            let rr = a.get(i + 1).copied().unwrap_or(0.0);
            let ol = b.get(i).copied().unwrap_or(0.0);
            let or_ = b.get(i + 1).copied().unwrap_or(0.0);
            let d = ((rl - ol).abs()).max((rr - or_).abs());
            eprintln!("{:>8}  {:>+12.6}  {:>+12.6}  {:>12.6}", frame, rl, ol, d);
        }
    }

    CompareResult {
        total_frames,
        compared_frames: total_frames,
        max_abs_diff: max_diff,
        rms_diff_db: rms_db,
        first_divergence_frame: first_div,
        sample_rate: 0, // filled in by caller
    }
}

// ── Offline renderer ─────────────────────────────────────────────────────────

const DECK_IDS: &[&str] = &["A", "B", "C", "D"];
const CHUNK: usize = 512;

fn render_session(
    session: &SessionFile,
    sample_rate: u32,
    reference_len: usize,
) -> Result<Vec<f32>, String> {
    let mut decks: HashMap<String, DeckState> = DECK_IDS
        .iter()
        .map(|&id| (id.to_string(), DeckState::empty(sample_rate)))
        .collect();
    let mut strips: HashMap<String, ChannelStrip> = DECK_IDS
        .iter()
        .map(|&id| (id.to_string(), ChannelStrip::new(sample_rate as f32)))
        .collect();
    let mut master_gain = crate::audio::DEFAULT_MASTER_GAIN;

    // The live audio engine applies commands on the next callback after they fire,
    // so recorded audio always starts `buffer_size_frames` after the event timestamp.
    // Read this from the recording_start event; default 512 (macOS Core Audio default).
    let buffer_latency = session
        .events
        .iter()
        .find(|e| e.event_type == "recording_start")
        .and_then(|e| e.buffer_size_frames)
        .unwrap_or(512) as usize;

    // Build sample-accurate event timeline, offset by buffer latency.
    let mut timeline: Vec<(usize, &SessionEvent)> = session
        .events
        .iter()
        .map(|ev| {
            let pos = (ev.elapsed_ms.max(0.0) * sample_rate as f64 / 1000.0).round() as usize;
            (pos + buffer_latency, ev)
        })
        .collect();
    timeline.sort_by_key(|(pos, _)| *pos);

    // Render at least as many frames as the reference, plus a small margin.
    let total_frames = (reference_len / 2).max(
        timeline
            .last()
            .map(|(p, _)| p + sample_rate as usize)
            .unwrap_or(0),
    );
    let mut output = vec![0.0f32; total_frames * 2];
    let mut limiter = LimiterState::new(sample_rate as f32);

    let mut ev_idx = 0;
    let mut frame = 0usize;

    while frame < total_frames {
        let chunk_end = (frame + CHUNK).min(total_frames);

        // Dispatch events whose position falls before this chunk's end.
        while ev_idx < timeline.len() && timeline[ev_idx].0 < chunk_end {
            let ev = timeline[ev_idx].1;
            if ev.event_type == "set_master_gain" {
                if let Some(g) = ev.gain {
                    master_gain = g.clamp(0.0, 1.0);
                }
            } else {
                apply_event(ev, &mut decks, &mut strips, sample_rate)?;
            }
            ev_idx += 1;
        }

        for f in frame..chunk_end {
            let mut mix_l = 0.0f32;
            let mut mix_r = 0.0f32;
            for id in DECK_IDS {
                if let (Some(deck), Some(strip)) = (decks.get_mut(*id), strips.get_mut(*id)) {
                    let (l, r) = deck.main_tick();
                    let (pl, pr) = strip.process_main(l, r);
                    mix_l += pl;
                    mix_r += pr;
                }
            }
            let (lim_l, lim_r) = limiter.process(mix_l * master_gain, mix_r * master_gain);
            output[f * 2] = lim_l;
            output[f * 2 + 1] = lim_r;
        }

        frame = chunk_end;
    }

    Ok(output)
}

fn apply_event(
    ev: &SessionEvent,
    decks: &mut HashMap<String, DeckState>,
    strips: &mut HashMap<String, ChannelStrip>,
    sr: u32,
) -> Result<(), String> {
    let sr_f = sr as f64;

    macro_rules! deck {
        ($id:expr) => {
            decks
                .get_mut($id)
                .ok_or_else(|| format!("unknown deck: {}", $id))?
        };
    }
    macro_rules! strip {
        ($id:expr) => {
            strips
                .get_mut($id)
                .ok_or_else(|| format!("unknown deck: {}", $id))?
        };
    }
    macro_rules! to_frames {
        ($sec:expr) => {
            ($sec * sr_f).max(0.0)
        };
    }

    match ev.event_type.as_str() {
        "deck_snapshot" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            let Some(ref path) = ev.path else {
                return Ok(());
            };
            let d = deck!(id);
            load_deck(d, path, sr)?;
            if let Some(pos) = ev.position_sec {
                let f = to_frames!(pos).min(d.total_frames as f64);
                d.main_pos = f;
                d.cue_pos = f;
            }
            if let Some(cp) = ev.cue_point_sec {
                d.cue_point = to_frames!(cp).min(d.total_frames as f64);
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
                d.loop_end = to_frames!(le).min(d.total_frames as f64);
            }
            if ev.is_playing == Some(true) {
                d.is_playing = true;
            }
        }

        "load_track" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            let Some(ref path) = ev.path else {
                return Ok(());
            };
            let d = deck!(id);
            load_deck(d, path, sr)?;
            if let Some(offset) = ev.beat_offset_sec {
                let f = (offset * sr_f).clamp(0.0, d.total_frames as f64);
                d.main_pos = f;
                d.cue_pos = f;
                d.cue_point = f;
            }
        }

        "eject_track" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            *deck!(id) = DeckState::empty(sr);
        }

        "play" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            let d = deck!(id);
            match ev.sec {
                Some(sec) => {
                    let f = to_frames!(sec).min(d.total_frames as f64);
                    d.main_pos = f;
                    d.cue_pos = f;
                }
                None => {
                    d.cue_pos = d.main_pos;
                }
            }
            d.is_playing = true;
        }

        "stop" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            deck!(id).is_playing = false;
        }

        "stopped_at_cue" | "stop_at_cue" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            let d = deck!(id);
            d.is_playing = false;
            if let Some(cp) = ev.cue_point_sec {
                let f = to_frames!(cp).min(d.total_frames as f64);
                d.main_pos = f;
                d.cue_pos = f;
            }
        }

        "seek" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            if let Some(sec) = ev.sec {
                let d = deck!(id);
                let f = to_frames!(sec).min(d.total_frames as f64);
                d.main_pos = f;
                d.cue_pos = f;
                if d.loop_active && (f < d.cue_point || f >= d.loop_end) {
                    d.loop_active = false;
                }
            }
        }

        "set_volume" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            if let Some(g) = ev.gain {
                strip!(id).set_gain(g);
            }
        }

        "set_eq" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            if let (Some(ref band), Some(db)) = (&ev.band, ev.db) {
                strip!(id).set_eq_band(band, db);
            }
        }

        "set_filter" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            if let Some(v) = ev.value {
                strip!(id).set_filter(v);
            }
        }

        "set_filter_active" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            if let Some(a) = ev.active {
                strip!(id).set_filter_active(a);
            }
        }

        "set_playback_rate" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            if let Some(r) = ev.rate {
                deck!(id).playback_rate = r.max(0.1);
            }
        }

        "set_nudge" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            if let Some(p) = ev.percent {
                deck!(id).nudge_factor = 1.0 + p / 100.0;
            }
        }

        "set_beat_grid" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            let d = deck!(id);
            if let Some(bpm) = ev.bpm {
                d.bpm = Some(bpm);
            }
            if let Some(off) = ev.beat_offset_sec {
                d.beat_offset_frames = off * sr_f;
            }
        }

        "loop_in" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            let d = deck!(id);
            if let Some(cs) = ev.cue_sec {
                d.cue_point = to_frames!(cs).min(d.total_frames as f64);
            }
            d.loop_active = false;
            d.loop_end = 0.0;
        }

        "loop_out" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            let d = deck!(id);
            if let Some(ss) = ev.start_sec {
                d.cue_point = to_frames!(ss).min(d.total_frames as f64);
            }
            if let Some(es) = ev.end_sec {
                d.loop_end = to_frames!(es).min(d.total_frames as f64);
            }
            d.loop_active = true;
        }

        "exit_loop" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            deck!(id).loop_active = false;
        }

        "reloop" => {
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            let d = deck!(id);
            if d.loop_end > d.cue_point {
                d.main_pos = d.cue_point;
                d.cue_pos = d.cue_point;
                if d.is_playing {
                    d.loop_active = true;
                }
            }
        }

        "cue_preview_start" => {
            // Holding CUE plays from the cue point through the main path — audible in the recording.
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            let d = deck!(id);
            let cp = ev.cue_point_sec.unwrap_or(d.cue_point / sr_f);
            let f = to_frames!(cp).min(d.total_frames as f64);
            d.cue_point = f;
            d.main_pos = f;
            d.cue_pos = f;
            d.is_playing = true;
            d.is_cueing = true;
        }

        "cue_preview_end" => {
            // Releasing CUE stops playback and returns to cue point.
            let Some(ref id) = ev.deck else {
                return Ok(());
            };
            let d = deck!(id);
            let cp = ev.cue_point_sec.unwrap_or(d.cue_point / sr_f);
            let f = to_frames!(cp).min(d.total_frames as f64);
            d.is_playing = false;
            d.is_cueing = false;
            d.main_pos = f;
            d.cue_pos = f;
        }

        // Deliberately not replayed: recording_start/stop, deck_snapshot (handled above),
        // cue_move, set_cue_active, set_master_gain, set_cue_mix.
        _ => {}
    }

    Ok(())
}

fn load_deck(deck: &mut DeckState, path: &str, sr: u32) -> Result<(), String> {
    let (raw, channels, native_sr) =
        audio::decode_audio(path).map_err(|e| format!("{path}: {e}"))?;
    let resampled = if native_sr == sr {
        raw
    } else {
        audio::resample_linear(&raw, channels, native_sr, sr)
    };
    let total_frames = resampled.len() / channels;
    deck.samples = Arc::new(resampled);
    deck.channels = channels;
    deck.device_sample_rate = sr;
    deck.total_frames = total_frames;
    deck.duration = total_frames as f64 / sr as f64;
    deck.is_playing = false;
    deck.is_cueing = false;
    deck.main_pos = 0.0;
    deck.cue_pos = 0.0;
    deck.cue_point = 0.0;
    deck.loop_active = false;
    deck.loop_end = 0.0;
    deck.bpm = None;
    deck.beat_offset_frames = 0.0;
    deck.playback_rate = 1.0;
    deck.nudge_factor = 1.0;
    Ok(())
}
