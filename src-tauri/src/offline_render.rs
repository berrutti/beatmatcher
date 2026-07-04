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

const DECK_IDS: &[&str] = &["A", "B", "C", "D"];
const CHUNK: usize = 512;

pub fn render_session(
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
                apply_event(cmd, &mut decks, &mut strips, &mut master_gain, sample_rate)?;
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
    cmd: SessionCommand<'_>,
    decks: &mut HashMap<String, DeckState>,
    strips: &mut HashMap<String, ChannelStrip>,
    master_gain: &mut f32,
    sr: u32,
) -> Result<(), String> {
    if let SessionCommand::SetMasterGain { gain } = cmd {
        *master_gain = gain.clamp(0.0, 1.0);
        return Ok(());
    }

    let id = cmd
        .deck_id()
        .expect("non-SetMasterGain commands target a deck");
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
