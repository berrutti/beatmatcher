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

pub fn write_flac_f32(path: &str, samples: &[f32], sample_rate: u32) -> Result<(), String> {
    use flacenc::bitsink::ByteSink;
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    use std::io::Write;

    const MAX_24BIT: f32 = 8_388_607.0;
    let source = SliceSource {
        samples: samples
            .iter()
            .map(|&s| (s.clamp(-1.0, 1.0) * MAX_24BIT) as i32)
            .collect(),
        channels: 2,
        bits_per_sample: 24,
        sample_rate: sample_rate as usize,
        pos: 0,
    };

    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|e| format!("FLAC config error: {e:?}"))?;
    let block_size = config.block_size;
    let stream = flacenc::encode_with_fixed_block_size(&config, source, block_size)
        .map_err(|e| format!("FLAC encode error: {e:?}"))?;

    let mut sink = ByteSink::with_capacity(stream.count_bits());
    stream
        .write(&mut sink)
        .map_err(|e| format!("FLAC write error: {e:?}"))?;

    std::fs::File::create(path)
        .and_then(|mut f| f.write_all(sink.as_slice()))
        .map_err(|e| format!("{path}: {e}"))
}

struct SliceSource {
    samples: Vec<i32>,
    channels: usize,
    bits_per_sample: usize,
    sample_rate: usize,
    pos: usize,
}

impl flacenc::source::Source for SliceSource {
    fn channels(&self) -> usize {
        self.channels
    }
    fn bits_per_sample(&self) -> usize {
        self.bits_per_sample
    }
    fn sample_rate(&self) -> usize {
        self.sample_rate
    }
    fn len_hint(&self) -> Option<usize> {
        Some(self.samples.len() / self.channels)
    }

    fn read_samples<F: flacenc::source::Fill>(
        &mut self,
        block_size: usize,
        dest: &mut F,
    ) -> Result<usize, flacenc::error::SourceError> {
        let n = (block_size * self.channels).min(self.samples.len() - self.pos);
        if n == 0 {
            return Ok(0);
        }
        dest.fill_interleaved(&self.samples[self.pos..self.pos + n])?;
        self.pos += n;
        Ok(n / self.channels)
    }
}

use session_core::event::SessionCommand;
pub use session_core::event::{SessionEvent, SessionFile};

pub struct CompareResult {
    pub total_frames: usize,
    pub compared_frames: usize,
    pub max_abs_diff: f32,
    pub rms_diff_db: f32,
    pub first_divergence_frame: Option<usize>,
    pub sample_rate: u32,
}

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
    let session = SessionFile::parse(&json).map_err(|e| format!("parse error: {e}"))?;

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

const DECK_IDS: &[&str] = &["A", "B", "C", "D"];
const CHUNK: usize = 512;

pub fn render_session(
    session: &SessionFile,
    sample_rate: u32,
    reference_len: usize,
) -> Result<Vec<f32>, String> {
    // Refused rather than rendered on a best guess: a mixer this build cannot
    // reproduce would diverge from the recording without saying so.
    let manifest = session_core::resolve_manifest(session.mixer.as_ref())?;

    let mut decks: HashMap<String, DeckState> = DECK_IDS
        .iter()
        .map(|&id| (id.to_string(), DeckState::empty(sample_rate)))
        .collect();
    let mut strips: HashMap<String, ChannelStrip> = DECK_IDS
        .iter()
        .map(|&id| {
            (
                id.to_string(),
                ChannelStrip::from_manifest(manifest, sample_rate as f32),
            )
        })
        .collect();
    let mut master_gain = crate::audio::DEFAULT_MASTER_GAIN;
    let mut xfader_position = 0.0f32;

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
    // A deck_snapshot is initial state; at a shared frame it must apply before
    // any other event, or it resets a deck a same-frame play already started.
    timeline.sort_by(|a, b| {
        a.0.cmp(&b.0).then_with(|| {
            u8::from(a.1.event_type != "deck_snapshot")
                .cmp(&u8::from(b.1.event_type != "deck_snapshot"))
        })
    });

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
            if let Some(cmd) = timeline[ev_idx].1.command() {
                apply_event(
                    cmd,
                    &mut decks,
                    &mut strips,
                    &mut master_gain,
                    &mut xfader_position,
                    sample_rate,
                )?;
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

fn resolve_xfader_gains(strips: &mut HashMap<String, ChannelStrip>, position: f32) {
    for strip in strips.values_mut() {
        strip.set_xfader_position(position);
    }
}

fn apply_event(
    cmd: SessionCommand<'_>,
    decks: &mut HashMap<String, DeckState>,
    strips: &mut HashMap<String, ChannelStrip>,
    master_gain: &mut f32,
    xfader_position: &mut f32,
    sr: u32,
) -> Result<(), String> {
    if let SessionCommand::SetParam {
        scope: session_core::ParamScope::Master,
        slot,
        param,
        value,
        ..
    } = cmd
    {
        if (slot, param) == ("gain", "gain") {
            *master_gain = (value as f32).clamp(0.0, 1.0);
        }
        if (slot, param) == ("xfader", "position") {
            *xfader_position = (value as f32).clamp(-1.0, 1.0);
            resolve_xfader_gains(strips, *xfader_position);
        }
        return Ok(());
    }

    if let SessionCommand::SetFaderCurve { curve } = cmd {
        for strip in strips.values_mut() {
            strip.set_fader_curve(curve);
        }
        return Ok(());
    }

    if let SessionCommand::SetJogRotationSpeed { speed } = cmd {
        for deck in decks.values_mut() {
            deck.set_jog_rotation_speed(speed);
        }
        return Ok(());
    }

    let id = cmd
        .deck_id()
        .expect("master-scope commands are handled above; the rest target a deck");
    let d = decks
        .get_mut(id)
        .ok_or_else(|| format!("unknown deck: {id}"))?;
    let s = strips
        .get_mut(id)
        .ok_or_else(|| format!("unknown deck: {id}"))?;

    let mut load_samples = |path: &str| -> Result<(Arc<Vec<f32>>, usize), String> {
        let (raw, channels, native_sr) =
            audio::decode_audio(path).map_err(|e| format!("{path}: {e}"))?;
        let resampled = if native_sr == sr {
            raw
        } else {
            audio::resample_linear(&raw, channels, native_sr, sr)
        };
        Ok((Arc::new(resampled), channels))
    };

    // The offline renderer never compensates for buffer-aligned overshoot
    // (no live audio thread to catch up with), so overshoot is always 0.
    audio::apply_deck_command(&cmd, d, s, sr, 0.0, &mut load_samples)
}

// The DSP tests elsewhere are property tests, so they still pass after a change
// to a filter Q or a reordered cascade. This pins the actual numbers.
#[cfg(test)]
mod golden {
    use super::*;

    pub(super) const SAMPLE_RATE: u32 = 44_100;
    // Prime, so probes never align with the chunk size or the filter's
    // coefficient refresh interval.
    const PROBE_STRIDE: usize = 997;
    // Loose enough to survive libm differences between architectures. Not a
    // bit-for-bit hash, so a platform difference reads as a near miss.
    const EPSILON: f32 = 1e-6;

    // Integer arithmetic only, so the stimulus is bit-identical everywhere;
    // broadband so any EQ or filter change shows up in the probes.
    fn source_samples(frames: usize) -> Vec<f32> {
        let mut state: u32 = 0x1234_5678;
        let mut out = Vec::with_capacity(frames * 2);
        for _ in 0..frames * 2 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            out.push(((state >> 8) as f32 / 8_388_608.0 - 1.0) * 0.6);
        }
        out
    }

    // Built once per process. Rewriting per call let one test decode a WAV another was
    // still writing. Written under a unique name and renamed in, so no partial file reads.
    pub(super) fn source_wav_path() -> String {
        static SOURCE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        SOURCE
            .get_or_init(|| {
                let dir = std::env::temp_dir();
                let final_path = dir.join("beatmatcher_golden_source.wav");
                let staging = dir.join(format!(
                    "beatmatcher_golden_source.{}.partial",
                    std::process::id()
                ));
                let staging = staging.to_string_lossy().to_string();
                write_wav_f32(
                    &staging,
                    &source_samples(SAMPLE_RATE as usize * 3),
                    SAMPLE_RATE,
                    2,
                )
                .expect("write golden source wav");
                std::fs::rename(&staging, &final_path).expect("publish golden source wav");
                final_path.to_string_lossy().to_string()
            })
            .clone()
    }

    // Touches every mixer param so the golden covers the whole strip.
    fn session_json(source: &str) -> String {
        format!(
            r#"{{"version":2,"events":[
{{"elapsed_ms":0,"type":"recording_start","buffer_size_frames":512}},
{{"elapsed_ms":0,"type":"load_track","deck":"A","path":"{source}"}},
{{"elapsed_ms":0,"type":"set_param","deck":"A","slot":"fader","param":"gain","value":0.9}},
{{"elapsed_ms":50,"type":"play","deck":"A","sec":0.0}},
{{"elapsed_ms":200,"type":"set_param","deck":"A","slot":"eq","param":"low","value":6.0}},
{{"elapsed_ms":300,"type":"set_param","deck":"A","slot":"eq","param":"mid","value":-8.0}},
{{"elapsed_ms":400,"type":"set_param","deck":"A","slot":"eq","param":"high","value":3.0}},
{{"elapsed_ms":600,"type":"set_param","deck":"A","slot":"filter","param":"value","value":-0.5}},
{{"elapsed_ms":600,"type":"set_param","deck":"A","slot":"filter","param":"active","value":1}},
{{"elapsed_ms":900,"type":"set_playback_rate","deck":"A","rate":1.03}},
{{"elapsed_ms":1100,"type":"set_param","slot":"gain","param":"gain","value":0.9}},
{{"elapsed_ms":1300,"type":"set_param","deck":"A","slot":"fader","param":"gain","value":0.4}},
{{"elapsed_ms":1500,"type":"stop","deck":"A"}}
]}}"#
        )
    }

    fn render_golden() -> Vec<f32> {
        let source = source_wav_path();
        let session: SessionFile =
            serde_json::from_str(&session_json(&source)).expect("parse golden session");
        render_session(&session, SAMPLE_RATE, 0).expect("render golden session")
    }

    fn probes(rendered: &[f32]) -> Vec<[f32; 2]> {
        (0..rendered.len() / 2)
            .step_by(PROBE_STRIDE)
            .map(|frame| [rendered[frame * 2], rendered[frame * 2 + 1]])
            .collect()
    }

    const EXPECTED_FRAMES: usize = 110762;

    #[rustfmt::skip]
    const EXPECTED: &[[f32; 2]] = &[
    [0.000000000e0, 0.000000000e0],
    [0.000000000e0, 0.000000000e0],
    [0.000000000e0, 0.000000000e0],
    [-2.451000959e-1, 5.901873484e-2],
    [-2.505692542e-1, 3.483803570e-1],
    [4.144902229e-1, 1.363246739e-1],
    [2.867051363e-1, -6.976687163e-2],
    [3.088264167e-1, 4.218139946e-1],
    [-4.852249473e-2, -3.039745092e-1],
    [2.889249027e-1, -4.354272783e-2],
    [2.175512761e-1, 8.366455138e-2],
    [7.661589980e-2, 7.354977727e-2],
    [-3.339681923e-1, -1.062664986e-1],
    [6.372825056e-2, -4.276291728e-1],
    [2.327685058e-1, -4.337161183e-1],
    [-1.450516731e-1, -2.980370224e-1],
    [3.238647580e-1, -1.438434273e-1],
    [2.921864092e-1, 1.848343611e-1],
    [-1.533872038e-1, 1.448842138e-1],
    [3.356818557e-1, -1.758697033e-1],
    [3.726332188e-1, -1.758898348e-1],
    [2.787356377e-1, -1.152877510e-1],
    [1.496326029e-1, 1.740391999e-1],
    [5.103135016e-3, -5.228232741e-1],
    [6.187922135e-2, -7.287115441e-4],
    [-2.032785118e-1, -2.200358361e-1],
    [-1.169534773e-1, -3.876072764e-1],
    [-4.612441957e-1, 1.024869755e-1],
    [1.054645702e-2, -1.027540639e-1],
    [-9.422796965e-2, -2.121517062e-2],
    [7.987920195e-2, -2.011877857e-2],
    [-5.580013618e-2, -2.091827430e-2],
    [-5.914475769e-3, -1.056312025e-2],
    [2.967280569e-3, -4.243732989e-2],
    [-1.239206363e-2, 5.251218006e-2],
    [-7.242294494e-3, -9.063800797e-3],
    [8.643057663e-4, -1.297821291e-2],
    [-1.833692379e-2, 4.707301408e-2],
    [6.107360870e-2, -6.488546729e-3],
    [-5.615364015e-2, -1.694514230e-2],
    [2.090674639e-2, -6.082290318e-3],
    [4.188980907e-2, -2.861808753e-3],
    [-4.320310056e-2, -1.394836977e-2],
    [5.384303629e-3, -4.473705590e-2],
    [-2.854429185e-2, 3.116499633e-2],
    [1.272700727e-2, 2.822212316e-2],
    [-5.851341411e-2, -3.880524263e-2],
    [1.213700883e-2, -2.658736426e-3],
    [5.584542081e-2, 7.480273489e-3],
    [1.773027703e-2, 8.946559392e-3],
    [-5.455503985e-2, -1.206981577e-2],
    [-3.180675954e-2, 6.665207446e-2],
    [-2.896897681e-2, -1.679952256e-2],
    [-3.488714993e-2, -1.414723694e-2],
    [-6.142280623e-2, -3.337088600e-2],
    [-1.985147409e-2, 2.203846350e-2],
    [-5.709733348e-3, -7.426121086e-2],
    [3.124684654e-2, 4.818338901e-2],
    [-3.373233601e-3, -2.613295987e-2],
    [-1.581236161e-2, -2.028466575e-2],
    [3.744208720e-3, 1.403369103e-2],
    [9.827684611e-3, 1.581091993e-2],
    [1.561063109e-3, -8.332787082e-3],
    [-1.586777344e-2, 1.346421801e-2],
    [2.419789135e-2, -5.017070100e-3],
    [-2.460696269e-3, 1.853706432e-3],
    [-4.308318254e-3, -3.021943197e-2],
    [-1.492889714e-4, 1.163298512e-4],
    [-8.061145067e-13, 1.532258803e-12],
    [2.539761297e-20, 1.171367012e-21],
    [7.659263055e-28, -2.893192148e-28],
    [1.295160451e-35, -6.732960296e-36],
    [5.465064011e-44, -7.847271400e-44],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    [-4.091791516e-43, 1.401298464e-45],
    ];

    #[test]
    fn golden_session_renders_identically() {
        let rendered = render_golden();
        assert_eq!(
            rendered.len() / 2,
            EXPECTED_FRAMES,
            "rendered length changed"
        );

        let actual = probes(&rendered);
        assert_eq!(actual.len(), EXPECTED.len(), "probe count changed");
        for (index, (got, want)) in actual.iter().zip(EXPECTED).enumerate() {
            let frame = index * PROBE_STRIDE;
            assert!(
                (got[0] - want[0]).abs() <= EPSILON && (got[1] - want[1]).abs() <= EPSILON,
                "frame {frame}: got [{:+e}, {:+e}], want [{:+e}, {:+e}]",
                got[0],
                got[1],
                want[0],
                want[1]
            );
        }
    }

    #[test]
    #[ignore = "generator, not a check: run with --ignored --nocapture to reprint DIGESTS"]
    fn print_corpus_digests() {
        for (name, json) in super::corpus::CORPUS {
            let digest = super::corpus::digest_of(name, json);
            println!(
                "    (\"{name}\", Digest {{ frames: {}, peak: {:.9e}, rms: {:.9e}, probes: [{}] }}),",
                digest.frames,
                digest.peak,
                digest.rms,
                digest
                    .probes
                    .iter()
                    .map(|value| format!("{value:.9e}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    #[test]
    #[ignore = "generator, not a check: run with --ignored --nocapture to reprint EXPECTED"]
    fn print_golden_table() {
        let rendered = render_golden();
        println!("frames={}", rendered.len() / 2);
        for [l, r] in probes(&rendered) {
            println!("    [{l:+.9e}, {r:+.9e}],");
        }
    }
}

// Each fixture carries `__SOURCE__` where a track path goes; the test
// substitutes a synthesized WAV so the corpus stays self-contained.
#[cfg(test)]
pub(crate) mod corpus {
    use super::golden::{source_wav_path, SAMPLE_RATE};
    use super::*;

    pub(crate) const CORPUS: &[(&str, &str)] = &[
        (
            "transport",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/corpus/transport.bms"
            )),
        ),
        (
            "loops",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/corpus/loops.bms"
            )),
        ),
        (
            "mixer",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/corpus/mixer.bms"
            )),
        ),
        (
            "rate_and_multideck",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/corpus/rate_and_multideck.bms"
            )),
        ),
    ];

    // Events that carry no audio consequence, so `command()` returning None for
    // them is correct rather than a dropped variant.
    const NOT_REPLAYED: &[&str] = &[
        "recording_start",
        "recording_stop",
        "cue_move",
        "set_cue_active",
        "set_cue_mix",
    ];

    const PROBE_COUNT: usize = 8;
    const EPSILON: f32 = 1e-6;

    pub(super) struct Digest {
        pub frames: usize,
        pub peak: f32,
        pub rms: f32,
        pub probes: [f32; PROBE_COUNT],
    }

    fn parse(name: &str, json: &str) -> SessionFile {
        let patched = json.replace("__SOURCE__", &source_wav_path());
        serde_json::from_str(&patched).unwrap_or_else(|err| panic!("{name}: parse failed: {err}"))
    }

    pub(super) fn digest_of(name: &str, json: &str) -> Digest {
        let session = parse(name, json);
        let rendered = render_session(&session, SAMPLE_RATE, 0)
            .unwrap_or_else(|err| panic!("{name}: render failed: {err}"));

        let frames = rendered.len() / 2;
        let peak = rendered.iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
        let rms = (rendered.iter().map(|s| (*s as f64).powi(2)).sum::<f64>()
            / rendered.len() as f64)
            .sqrt() as f32;

        let mut probes = [0.0f32; PROBE_COUNT];
        for (index, probe) in probes.iter_mut().enumerate() {
            *probe = rendered[(frames / (PROBE_COUNT + 1)) * (index + 1) * 2];
        }
        Digest {
            frames,
            peak,
            rms,
            probes,
        }
    }

    #[rustfmt::skip]
    const DIGESTS: &[(&str, Digest)] = &[
    ("transport", Digest { frames: 146042, peak: 4.050964117e-1, rms: 1.623620540e-1, probes: [1.472007632e-1, -3.464962840e-1, 0.000000000e0, 1.398182660e-1, 2.248586267e-1, 0.000000000e0, 0.000000000e0, 0.000000000e0] }),
    ("loops", Digest { frames: 176912, peak: 3.812676072e-1, rms: 1.841667891e-1, probes: [-2.260529250e-1, -2.055428736e-2, 1.592096984e-1, -2.008791417e-1, -3.016780913e-1, 1.546460688e-1, 0.000000000e0, 0.000000000e0] }),
    ("mixer", Digest { frames: 154862, peak: 8.452718854e-1, rms: 1.412521601e-1, probes: [-1.562566310e-1, -1.870270371e-1, 7.054805011e-2, 2.928561844e-5, -1.448095292e-1, -1.325848047e-3, -2.185082994e-18, 1.177177433e-29] }),
    ("rate_and_multideck", Digest { frames: 146042, peak: 6.129323840e-1, rms: 1.628303677e-1, probes: [7.014070451e-2, 1.297436953e-1, 1.324779540e-2, 9.184795618e-2, -3.289961070e-2, 0.000000000e0, 0.000000000e0, 0.000000000e0] }),
    ];

    #[test]
    fn every_event_maps_to_a_command() {
        for (name, json) in CORPUS {
            let session = parse(name, json);
            assert!(!session.events.is_empty(), "{name}: no events");
            for event in &session.events {
                if NOT_REPLAYED.contains(&event.event_type.as_str()) {
                    continue;
                }
                assert!(
                    event.command().is_some(),
                    "{name}: '{}' at {} ms no longer maps to a SessionCommand",
                    event.event_type,
                    event.elapsed_ms
                );
            }
        }
    }

    #[test]
    fn corpus_renders_identically() {
        for (name, json) in CORPUS {
            let expected = DIGESTS
                .iter()
                .find(|(id, _)| id == name)
                .map(|(_, digest)| digest)
                .unwrap_or_else(|| panic!("{name}: no expected digest"));
            let actual = digest_of(name, json);

            assert_eq!(
                actual.frames, expected.frames,
                "{name}: frame count changed"
            );
            assert!(
                (actual.peak - expected.peak).abs() <= EPSILON,
                "{name}: peak {:e} != {:e}",
                actual.peak,
                expected.peak
            );
            assert!(
                (actual.rms - expected.rms).abs() <= EPSILON,
                "{name}: rms {:e} != {:e}",
                actual.rms,
                expected.rms
            );
            for (index, (got, want)) in actual.probes.iter().zip(&expected.probes).enumerate() {
                assert!(
                    (got - want).abs() <= EPSILON,
                    "{name}: probe {index} {got:e} != {want:e}"
                );
            }
        }
    }
}

// Guards on the param axis now that `set_param` is the only spelling.
#[cfg(test)]
mod param_addressing {
    use super::golden::{source_wav_path, SAMPLE_RATE};
    use super::*;

    fn render(events: &str) -> Vec<f32> {
        let json = format!(r#"{{"version":2,"events":[{events}]}}"#)
            .replace("__SOURCE__", &source_wav_path());
        let session: SessionFile = serde_json::from_str(&json).expect("parse session");
        render_session(&session, SAMPLE_RATE, 0).expect("render session")
    }

    const PREAMBLE: &str = r#"
{"elapsed_ms":0,"type":"recording_start","buffer_size_frames":512},
{"elapsed_ms":0,"type":"load_track","deck":"A","path":"__SOURCE__"},
{"elapsed_ms":50,"type":"play","deck":"A","sec":0.0},"#;

    const FADER_DOWN: &str = r#"{"elapsed_ms":100,"type":"set_param","deck":"A","slot":"fader","param":"gain","value":0.3},"#;
    const STOP: &str = r#"{"elapsed_ms":900,"type":"stop","deck":"A"}"#;

    const ASSIGN_A: &str =
        r#"{"elapsed_ms":100,"type":"set_xfader_assign","deck":"A","assign":"a"},"#;
    const XFADER_RIGHT: &str =
        r#"{"elapsed_ms":110,"type":"set_param","slot":"xfader","param":"position","value":1.0},"#;

    // The renderer has to honour the crossfader, or a recorded set renders as
    // something the DJ never played.
    #[test]
    fn a_deck_crossfaded_away_renders_silent() {
        let open = render(&format!("{PREAMBLE}{ASSIGN_A}{STOP}"));
        let cut = render(&format!("{PREAMBLE}{ASSIGN_A}{XFADER_RIGHT}{STOP}"));
        let sample_at = |ms: usize| ms * SAMPLE_RATE as usize / 1000 * 2;
        let window_rms = |frames: &[f32]| {
            let window = &frames[sample_at(400)..sample_at(880)];
            (window.iter().map(|sample| sample * sample).sum::<f32>() / window.len() as f32).sqrt()
        };
        assert!(
            window_rms(&cut) < window_rms(&open) * 0.01,
            "the crossfader did not cut: {} vs {}",
            window_rms(&cut),
            window_rms(&open)
        );
    }

    // Thru is the default, so a crossfader move with nothing assigned to it must
    // leave the render bit-identical. This is what makes the v2 mixer safe.
    #[test]
    fn a_crossfader_move_with_nothing_assigned_changes_nothing() {
        let without = render(&format!("{PREAMBLE}{STOP}"));
        let with_move = render(&format!("{PREAMBLE}{XFADER_RIGHT}{STOP}"));
        assert_eq!(without, with_move);
    }

    // The order the two arrive in must not matter: assigning after the fader has
    // already moved has to resolve against where it currently sits.
    #[test]
    fn assigning_after_the_crossfader_moved_still_cuts() {
        let assign_late =
            r#"{"elapsed_ms":200,"type":"set_xfader_assign","deck":"A","assign":"a"},"#;
        let cut = render(&format!("{PREAMBLE}{XFADER_RIGHT}{assign_late}{STOP}"));
        let sample_at = |ms: usize| ms * SAMPLE_RATE as usize / 1000 * 2;
        let window = &cut[sample_at(400)..sample_at(880)];
        let rms = (window.iter().map(|s| s * s).sum::<f32>() / window.len() as f32).sqrt();
        assert!(rms < 0.001, "a late assign did not resolve: {rms}");
    }

    // An unknown param must be inert, not fatal: a session recorded against a
    // mixer this build does not have still has to replay everything else.
    #[test]
    fn an_unknown_param_is_ignored_rather_than_failing_the_render() {
        let without = render(&format!("{PREAMBLE}{FADER_DOWN}{STOP}"));
        let with_unknown = render(&format!(
            r#"{PREAMBLE}{FADER_DOWN}
{{"elapsed_ms":300,"type":"set_param","deck":"A","slot":"reverb","param":"mix","value":0.5}},
{STOP}"#
        ));
        assert_eq!(without, with_unknown, "an unknown param changed the render");
    }

    #[test]
    fn a_session_recorded_on_a_mixer_this_build_lacks_is_refused() {
        let json = format!(
            r#"{{"version":2,"mixer":{{"id":"isolator","hash":"deadbeefdeadbeef"}},"events":[{PREAMBLE}{STOP}]}}"#
        )
        .replace("__SOURCE__", &source_wav_path());
        let session: SessionFile = serde_json::from_str(&json).expect("parse session");
        let error = render_session(&session, SAMPLE_RATE, 0).expect_err("should refuse");
        assert!(error.contains("isolator"), "{error}");
    }

    // The renderer builds the strip from the resolved manifest, so the same events on two
    // mixers must differ, or the header is being read and then ignored.
    #[test]
    fn the_resolved_manifest_is_the_one_the_renderer_builds() {
        let render_on = |manifest: &session_core::MixerManifest| {
            let header = serde_json::to_string(&manifest.header()).expect("header");
            let json = format!(
                r#"{{"version":2,"mixer":{header},"events":[{PREAMBLE}
{{"elapsed_ms":100,"type":"set_param","deck":"A","slot":"eq","param":"low","value":0.0}},
{STOP}]}}"#
            )
            .replace("__SOURCE__", &source_wav_path());
            let session: SessionFile = serde_json::from_str(&json).expect("parse session");
            render_session(&session, SAMPLE_RATE, 0).expect("render")
        };
        // 0.0 is a flat shelf on the classic mixer and a full kill on the
        // isolator, so the same event has to produce different audio.
        assert_ne!(
            render_on(&session_core::CLASSIC_3BAND),
            render_on(&session_core::ISOLATOR_3BAND)
        );
    }

    #[test]
    fn a_session_stamped_with_the_current_mixer_renders() {
        let header = serde_json::to_string(&session_core::CLASSIC_3BAND.header()).expect("header");
        let stamped = format!(r#"{{"version":2,"mixer":{header},"events":[{PREAMBLE}{STOP}]}}"#)
            .replace("__SOURCE__", &source_wav_path());
        let session: SessionFile = serde_json::from_str(&stamped).expect("parse session");
        assert_eq!(
            render_session(&session, SAMPLE_RATE, 0).expect("render"),
            render(&format!("{PREAMBLE}{STOP}")),
            "the header changed the render"
        );
    }

    // Without this the comparison above would hold trivially on two renders
    // where nothing the fader does is audible.
    #[test]
    fn the_fader_param_in_that_comparison_actually_moves_the_output() {
        let quiet = render(&format!("{PREAMBLE}{FADER_DOWN}{STOP}"));
        let full = render(&format!("{PREAMBLE}{STOP}"));
        // Windowed between the fader move and the stop: before 100 ms both
        // renders are at full gain, and after 900 ms both are silent.
        let sample_at = |ms: usize| ms * SAMPLE_RATE as usize / 1000 * 2;
        let window_rms = |frames: &[f32]| {
            let window = &frames[sample_at(400)..sample_at(880)];
            (window.iter().map(|sample| sample * sample).sum::<f32>() / window.len() as f32).sqrt()
        };
        assert!(
            window_rms(&quiet) < window_rms(&full) * 0.5,
            "fader move was inaudible: {} vs {}",
            window_rms(&quiet),
            window_rms(&full)
        );
    }
}
