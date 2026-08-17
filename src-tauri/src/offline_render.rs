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
    limiter: MasterLimiter,
) -> Result<CompareResult, String> {
    let (reference, sample_rate, ref_channels) = read_wav_f32(reference_path)?;
    if ref_channels != 2 {
        return Err(format!(
            "reference WAV must be stereo (got {ref_channels} channels)"
        ));
    }

    let json = std::fs::read_to_string(session_path).map_err(|e| format!("{session_path}: {e}"))?;
    let session = SessionFile::parse(&json).map_err(|e| format!("parse error: {e}"))?;

    let rendered = render_session(
        &session,
        RenderRequest {
            sample_rate,
            min_frames: reference.len() / 2,
            limiter,
        },
    )?;

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

/// Named so a call site cannot silently pass the wrong flag: the two branches are
/// different master chains, not a feature being switched off.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MasterLimiter {
    On,
    Off,
}

pub struct RenderRequest {
    pub sample_rate: u32,
    /// Render at least this many frames, so a comparison covers the whole reference.
    pub min_frames: usize,
    pub limiter: MasterLimiter,
}

/// The decks and strips a session plays through, and the master chain they mix into.
struct Mixer {
    decks: HashMap<String, DeckState>,
    strips: HashMap<String, ChannelStrip>,
    master_gain: f32,
    xfader_position: f32,
    limiter: Option<LimiterState>,
    sample_rate: u32,
}

impl Mixer {
    fn new(session: &SessionFile, request: &RenderRequest) -> Result<Self, String> {
        // Refused rather than rendered on a best guess: a mixer this build cannot
        // reproduce would diverge from the recording without saying so.
        let manifest = session_core::resolve_manifest(session.mixer.as_ref())?;
        let sample_rate = request.sample_rate;
        Ok(Self {
            decks: crate::audio::LIVE_DECK_IDS
                .iter()
                .map(|&id| (id.to_string(), DeckState::empty(sample_rate)))
                .collect(),
            strips: crate::audio::LIVE_DECK_IDS
                .iter()
                .map(|&id| {
                    (
                        id.to_string(),
                        ChannelStrip::from_manifest(manifest, sample_rate as f32),
                    )
                })
                .collect(),
            master_gain: crate::audio::DEFAULT_MASTER_GAIN,
            xfader_position: 0.0,
            limiter: (request.limiter == MasterLimiter::On)
                .then(|| LimiterState::new(sample_rate as f32)),
            sample_rate,
        })
    }

    fn apply(&mut self, cmd: SessionCommand<'_>) -> Result<(), String> {
        apply_event(
            cmd,
            &mut self.decks,
            &mut self.strips,
            &mut self.master_gain,
            &mut self.xfader_position,
            self.sample_rate,
        )
    }

    fn tick(&mut self) -> (f32, f32) {
        let mut mix_l = 0.0f32;
        let mut mix_r = 0.0f32;
        for id in crate::audio::LIVE_DECK_IDS {
            if let (Some(deck), Some(strip)) = (self.decks.get_mut(id), self.strips.get_mut(id)) {
                deck.consume_jog(1);
                let (deck_l, deck_r) = deck.main_tick();
                let (strip_l, strip_r) = strip.process_main(deck_l, deck_r);
                mix_l += strip_l;
                mix_r += strip_r;
            }
        }
        crate::audio::master_output(
            self.limiter.as_mut(),
            mix_l * self.master_gain,
            mix_r * self.master_gain,
        )
    }
}

/// Every event dispatches at a frame. A recorded one carries the output frame the live
/// engine applied it at; a synthesized one has no live moment to recover, so its
/// timestamp is the position, converted and not rounded to anything.
fn build_timeline(session: &SessionFile, sample_rate: u32) -> Vec<(usize, &SessionEvent)> {
    let mut timeline: Vec<(usize, &SessionEvent)> = session
        .events
        .iter()
        .map(|ev| {
            let pos = match ev.frame {
                Some(frame) => frame as usize,
                None => (ev.elapsed_ms.max(0.0) * sample_rate as f64 / 1000.0).round() as usize,
            };
            (pos, ev)
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
    timeline
}

fn render_timeline(
    mixer: &mut Mixer,
    timeline: &[(usize, &SessionEvent)],
    total_frames: usize,
) -> Result<Vec<f32>, String> {
    let mut output = vec![0.0f32; total_frames * 2];
    let mut next_event = 0;
    let mut frame = 0usize;

    while frame < total_frames {
        while next_event < timeline.len() && timeline[next_event].0 <= frame {
            if let Some(cmd) = timeline[next_event].1.command() {
                mixer.apply(cmd)?;
            }
            next_event += 1;
        }

        let chunk_end = timeline
            .get(next_event)
            .map_or(total_frames, |(pos, _)| *pos)
            .min(total_frames);

        for output_frame in frame..chunk_end {
            let (l, r) = mixer.tick();
            output[output_frame * 2] = l;
            output[output_frame * 2 + 1] = r;
        }

        frame = chunk_end;
    }

    Ok(output)
}

pub fn render_session(session: &SessionFile, request: RenderRequest) -> Result<Vec<f32>, String> {
    let mut mixer = Mixer::new(session, &request)?;
    let timeline = build_timeline(session, request.sample_rate);

    // Plus a second of tail so a decay is never truncated.
    let tail_from = timeline.last().map_or(0, |(pos, _)| *pos);
    let total_frames = request
        .min_frames
        .max(tail_from + request.sample_rate as usize);

    render_timeline(&mut mixer, &timeline, total_frames)
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
        render_session(
            &session,
            RenderRequest {
                sample_rate: SAMPLE_RATE,
                min_frames: 0,
                limiter: MasterLimiter::On,
            },
        )
        .expect("render golden session")
    }

    fn probes(rendered: &[f32]) -> Vec<[f32; 2]> {
        (0..rendered.len() / 2)
            .step_by(PROBE_STRIDE)
            .map(|frame| [rendered[frame * 2], rendered[frame * 2 + 1]])
            .collect()
    }

    const EXPECTED_FRAMES: usize = 110250;

    #[rustfmt::skip]
    const EXPECTED: &[[f32; 2]] = &[
        [0e0, 0e0],
        [0e0, 0e0],
        [0e0, 0e0],
        [-3.2688314e-1, -2.8689215e-1],
        [-1.7820081e-1, -1.3198644e-1],
        [-2.9432967e-1, 1.796442e-1],
        [2.6306394e-1, -3.1698087e-1],
        [2.7785844e-1, 2.5995547e-1],
        [1.19956106e-1, 9.051184e-2],
        [-3.4444642e-1, -1.9433258e-1],
        [-1.1272958e-1, -1.5034916e-1],
        [-4.34953e-2, 4.0885064e-1],
        [-4.0797862e-1, 1.0387305e-1],
        [-3.1113583e-1, 4.1604045e-1],
        [9.453289e-2, 3.988101e-1],
        [2.4684455e-1, -1.467382e-1],
        [-1.2649426e-1, -4.1618858e-2],
        [8.4187895e-2, -4.5606866e-2],
        [4.0311837e-1, -1.5946354e-1],
        [2.6211578e-1, 1.403633e-1],
        [-5.1981694e-1, -3.8707575e-1],
        [1.0013949e-1, 3.0669987e-1],
        [-3.1847298e-1, -2.9300603e-1],
        [6.624613e-1, -2.8951975e-2],
        [-5.504219e-1, 3.366386e-1],
        [2.4361841e-1, -1.8195681e-1],
        [-2.7261797e-1, -2.1407054e-1],
        [-2.4786117e-2, 3.9481632e-2],
        [2.378092e-1, 4.168314e-2],
        [-3.867634e-2, -7.499512e-2],
        [-5.4630734e-2, 6.265101e-2],
        [5.2529544e-2, -1.5084036e-1],
        [-6.830194e-2, -3.1637726e-3],
        [3.210717e-2, 3.8821388e-2],
        [8.302816e-3, 6.4336704e-3],
        [8.7451e-3, 4.4180702e-2],
        [4.9616005e-3, 4.516171e-2],
        [1.9346999e-2, -2.8697213e-2],
        [-2.9477507e-3, 1.2615366e-3],
        [2.565301e-2, -3.6690456e-3],
        [5.347026e-2, 1.29902195e-2],
        [7.1496926e-3, -3.238475e-2],
        [4.7561888e-2, 2.7933057e-2],
        [4.0670508e-3, 7.650873e-2],
        [3.1063432e-2, 2.1795638e-2],
        [5.6275995e-3, -9.213159e-3],
        [4.4673537e-3, 1.6097106e-2],
        [-1.9113488e-2, -8.832177e-3],
        [2.0083334e-2, 7.1130255e-3],
        [1.5871154e-2, 2.5810203e-2],
        [-5.017438e-3, -3.8550207e-3],
        [-8.932088e-2, 1.9584265e-2],
        [-5.773527e-2, 3.852888e-3],
        [1.2124234e-1, 4.413637e-2],
        [-9.257765e-3, -1.9677222e-2],
        [1.3516799e-2, 3.6217444e-2],
        [9.1687925e-3, -2.2575619e-2],
        [-9.23872e-3, 5.0417304e-2],
        [1.373919e-2, 4.3017273e-3],
        [1.1392089e-2, 8.7756e-4],
        [2.932046e-2, -1.0795273e-2],
        [6.4321345e-4, -1.2843515e-2],
        [2.978315e-3, 6.5846095e-4],
        [-1.1329926e-2, -5.4054824e-3],
        [-2.035909e-2, -3.287505e-2],
        [1.6288932e-2, 2.4078498e-2],
        [-1.2516787e-2, -2.0530168e-3],
        [7.496227e-8, 2.7248444e-8],
        [1.6613806e-15, 3.6057335e-16],
        [2.0601945e-23, 1.8120938e-24],
        [1.6220977e-31, -2.627784e-32],
        [2.61746e-40, -8.79141e-40],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
        [4.26e-43, -4.04e-43],
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
        let rendered = render_session(
            &session,
            RenderRequest {
                sample_rate: SAMPLE_RATE,
                min_frames: 0,
                limiter: MasterLimiter::On,
            },
        )
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
    ("transport", Digest { frames: 145530, peak: 4.050901532e-1, rms: 1.631384939e-1, probes: [-2.738414407e-1, -9.195664525e-2, 0.000000000e0, 1.988919973e-1, 1.242127642e-2, 0.000000000e0, 0.000000000e0, 0.000000000e0] }),
    ("loops", Digest { frames: 176400, peak: 3.812613487e-1, rms: 1.845077127e-1, probes: [5.432673171e-2, 1.991462111e-1, 1.169061381e-2, 3.311527371e-1, -1.449688673e-1, 1.871924698e-1, 0.000000000e0, 0.000000000e0] }),
    ("mixer", Digest { frames: 154350, peak: 8.726367950e-1, rms: 1.416834742e-1, probes: [2.880797386e-1, -1.891948432e-1, 1.530065667e-2, 5.541073187e-5, -3.012379408e-1, 2.342810482e-2, 5.901104008e-18, -1.350032203e-29] }),
    ("rate_and_multideck", Digest { frames: 145530, peak: 6.103462577e-1, rms: 1.672426015e-1, probes: [-8.195396513e-2, -3.098741472e-1, 1.221538559e-1, 9.315679222e-2, -7.356053591e-2, 0.000000000e0, 0.000000000e0, 0.000000000e0] }),
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
        render_session(
            &session,
            RenderRequest {
                sample_rate: SAMPLE_RATE,
                min_frames: 0,
                limiter: MasterLimiter::On,
            },
        )
        .expect("render session")
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
        let error = render_session(
            &session,
            RenderRequest {
                sample_rate: SAMPLE_RATE,
                min_frames: 0,
                limiter: MasterLimiter::On,
            },
        )
        .expect_err("should refuse");
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
            render_session(
                &session,
                RenderRequest {
                    sample_rate: SAMPLE_RATE,
                    min_frames: 0,
                    limiter: MasterLimiter::On,
                },
            )
            .expect("render")
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
            render_session(
                &session,
                RenderRequest {
                    sample_rate: SAMPLE_RATE,
                    min_frames: 0,
                    limiter: MasterLimiter::On
                }
            )
            .expect("render"),
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

// The .bms records no limiter setting, so the renderer takes it from the caller.
// It diverged from the live path once by always limiting; these pin both branches.
#[cfg(test)]
mod master_limiter {
    use super::golden::{source_wav_path, SAMPLE_RATE};
    use super::*;

    // Noise at 0.6 through both shelves at their +6 dB maximum, at full master
    // gain, drives the mix past full scale.
    fn loud_session() -> SessionFile {
        let json = r#"{"version":2,"events":[
{"elapsed_ms":0,"type":"recording_start","buffer_size_frames":512},
{"elapsed_ms":0,"type":"load_track","deck":"A","path":"__SOURCE__"},
{"elapsed_ms":0,"type":"set_param","slot":"gain","param":"gain","value":1.0},
{"elapsed_ms":0,"type":"set_param","deck":"A","slot":"eq","param":"low","value":6.0},
{"elapsed_ms":0,"type":"set_param","deck":"A","slot":"eq","param":"high","value":6.0},
{"elapsed_ms":50,"type":"play","deck":"A","sec":0.0},
{"elapsed_ms":400,"type":"stop","deck":"A"}
]}"#
        .replace("__SOURCE__", &source_wav_path());
        serde_json::from_str(&json).expect("parse session")
    }

    fn peak(rendered: &[f32]) -> f32 {
        rendered
            .iter()
            .fold(0.0f32, |acc, sample| acc.max(sample.abs()))
    }

    #[test]
    fn a_disabled_limiter_hard_clips_at_full_scale() {
        let rendered = render_session(
            &loud_session(),
            RenderRequest {
                sample_rate: SAMPLE_RATE,
                min_frames: 0,
                limiter: MasterLimiter::Off,
            },
        )
        .expect("render");
        assert_eq!(peak(&rendered), 1.0);
    }

    #[test]
    fn an_enabled_limiter_holds_the_mix_under_its_threshold() {
        let rendered = render_session(
            &loud_session(),
            RenderRequest {
                sample_rate: SAMPLE_RATE,
                min_frames: 0,
                limiter: MasterLimiter::On,
            },
        )
        .expect("render");
        assert!(
            peak(&rendered) <= LimiterState::THRESHOLD + 1e-6,
            "peaked at {}",
            peak(&rendered)
        );
    }

    #[test]
    fn the_setting_changes_the_rendered_audio() {
        let limited = render_session(
            &loud_session(),
            RenderRequest {
                sample_rate: SAMPLE_RATE,
                min_frames: 0,
                limiter: MasterLimiter::On,
            },
        )
        .expect("render");
        let clipped = render_session(
            &loud_session(),
            RenderRequest {
                sample_rate: SAMPLE_RATE,
                min_frames: 0,
                limiter: MasterLimiter::Off,
            },
        )
        .expect("render");
        assert_ne!(limited, clipped);
    }
}

// The recorded output-frame count is the buffer the command actually landed in.
// Inferring it from the timestamp is out by one buffer whenever a boundary falls
// between the engine being mutated and the event being logged.
#[cfg(test)]
mod recorded_frame {
    use super::golden::{source_wav_path, SAMPLE_RATE};
    use super::*;

    fn first_audible_frame(rendered: &[f32]) -> usize {
        rendered
            .iter()
            .position(|sample| *sample != 0.0)
            .expect("render is silent")
            / 2
    }

    fn render_with_play(play_frame: Option<u64>, buffer_size_frames: u32) -> Vec<f32> {
        let stamp = match play_frame {
            Some(frame) => format!(r#","frame":{frame}"#),
            None => String::new(),
        };
        let json = format!(
            r#"{{"version":2,"events":[
{{"elapsed_ms":0,"type":"recording_start","buffer_size_frames":{buffer_size_frames}}},
{{"elapsed_ms":0,"type":"load_track","deck":"A","path":"__SOURCE__"}},
{{"elapsed_ms":50,"type":"play","deck":"A"{stamp}}},
{{"elapsed_ms":900,"type":"stop","deck":"A"}}
]}}"#
        )
        .replace("__SOURCE__", &source_wav_path());
        let session: SessionFile = serde_json::from_str(&json).expect("parse session");
        render_session(
            &session,
            RenderRequest {
                sample_rate: SAMPLE_RATE,
                min_frames: 0,
                limiter: MasterLimiter::On,
            },
        )
        .expect("render")
    }

    #[test]
    fn a_stamped_frame_is_used_verbatim() {
        assert_eq!(
            first_audible_frame(&render_with_play(Some(7000), 128)),
            7000
        );
    }

    // The stamp records where the command landed, so nothing about it may be
    // re-derived from the buffer the session happened to run at.
    #[test]
    fn a_stamped_frame_ignores_the_buffer_size() {
        for buffer_size_frames in [64u32, 128, 512, 1024] {
            assert_eq!(
                first_audible_frame(&render_with_play(Some(7000), buffer_size_frames)),
                7000,
                "buffer {buffer_size_frames}"
            );
        }
    }

    // Guards the two above against a stamp that happens to agree with the timestamp.
    #[test]
    fn an_unstamped_event_dispatches_at_its_timestamp() {
        let nominal = (50.0 * SAMPLE_RATE as f64 / 1000.0).round() as usize;
        assert_eq!(first_audible_frame(&render_with_play(None, 128)), nominal);
        assert_ne!(nominal, 7000);
    }
}

#[cfg(test)]
mod jog {
    use super::golden::{source_wav_path, SAMPLE_RATE};
    use super::*;

    // 500 ticks at 0.002 s each, bent by JOG_PAUSED_MULTIPLIER on a playing deck:
    // 500 * 0.002 * 44100 / 100 lands on exactly 441 frames.
    const EXPECTED_TRAVEL: i64 = 441;

    fn render_with(jog: &str) -> Vec<f32> {
        let json = format!(
            r#"{{"version":2,"events":[
{{"elapsed_ms":0,"type":"recording_start","buffer_size_frames":512}},
{{"elapsed_ms":0,"type":"load_track","deck":"A","path":"__SOURCE__"}},
{{"elapsed_ms":50,"type":"play","deck":"A"}}{jog},
{{"elapsed_ms":2500,"type":"stop","deck":"A"}}
]}}"#
        )
        .replace("__SOURCE__", &source_wav_path());
        let session: SessionFile = serde_json::from_str(&json).expect("parse session");
        render_session(
            &session,
            RenderRequest {
                sample_rate: SAMPLE_RATE,
                min_frames: 0,
                limiter: MasterLimiter::On,
            },
        )
        .expect("render")
    }

    fn best_travel(jogged: &[f32], plain: &[f32], start_frame: usize, frames: usize) -> i64 {
        let mut best = 0;
        let mut best_error = f64::INFINITY;
        for shift in -2000i64..=2000 {
            let mut error = 0.0;
            for frame in (0..frames).step_by(4) {
                let left = jogged[(start_frame + frame) * 2];
                let index = (start_frame + frame) as i64 + shift;
                let right = plain[index as usize * 2];
                error += f64::from(left - right).powi(2);
            }
            if error < best_error {
                best_error = error;
                best = shift;
            }
        }
        best
    }

    #[test]
    fn a_jog_on_a_playing_deck_moves_the_render_by_its_travel() {
        let jogged = render_with(r#",{"elapsed_ms":200,"type":"jog","deck":"A","ticks":500}"#);
        let plain = render_with("");
        let settled = (SAMPLE_RATE as usize) * 3 / 4;
        assert_eq!(
            best_travel(&jogged, &plain, settled, 16384),
            EXPECTED_TRAVEL
        );
    }
}

#[cfg(test)]
mod jog_block_size {
    use super::golden::{source_wav_path, SAMPLE_RATE};
    use super::*;

    fn render_at_buffer(buffer_size_frames: u32) -> Vec<f32> {
        let json = format!(
            r#"{{"version":2,"events":[
{{"elapsed_ms":0,"type":"recording_start","buffer_size_frames":{buffer_size_frames}}},
{{"elapsed_ms":0,"type":"load_track","deck":"A","path":"__SOURCE__"}},
{{"elapsed_ms":50,"type":"play","deck":"A"}},
{{"elapsed_ms":200,"type":"jog","deck":"A","ticks":500}},
{{"elapsed_ms":2500,"type":"stop","deck":"A"}}
]}}"#
        )
        .replace("__SOURCE__", &source_wav_path());
        let session: SessionFile = serde_json::from_str(&json).expect("parse session");
        render_session(
            &session,
            RenderRequest {
                sample_rate: SAMPLE_RATE,
                min_frames: 0,
                limiter: MasterLimiter::On,
            },
        )
        .expect("render")
    }

    #[test]
    fn the_recorded_block_size_does_not_shape_the_wheel() {
        assert_eq!(render_at_buffer(128), render_at_buffer(1024));
    }
}

#[cfg(test)]
mod live_parity {
    use super::golden::{source_wav_path, SAMPLE_RATE};
    use super::*;
    use crate::audio::RenderTargets;

    const BLOCK: usize = 128;
    const PLAY_FRAME: usize = 256;
    const JOG_FRAME: usize = 1280;
    const SCRUB_FRAME: usize = 256;
    const SCRUB_PLAY_FRAME: usize = 22_050;
    const STOP_FRAME: usize = 22_050;
    // Past `SETTLE_SECONDS` of silence, so the live path skips whole blocks before this.
    const RESUME_FRAME: usize = 88_200;
    const TICKS: f64 = 500.0;
    const BLOCKS: usize = 900;

    const SCHEDULES: [&[usize]; 5] = [
        &[BLOCK],
        &[117, 118, 118, 117, 118],
        &[64],
        &[512],
        &[61, 512, 128, 7, 1024, 199],
    ];

    #[derive(Clone, Copy)]
    enum Cue {
        Play,
        Stop,
        Jog,
    }

    // One shared buffer across the decks rather than a scratch each: `render_block` accumulates
    // into it, which is the same sequence of additions `mix_frame` performs on the device buffer.
    fn live_render(sizes: &[usize], script: &[(usize, &str, Cue)]) -> (Vec<f32>, Vec<usize>) {
        let mut fired = vec![0usize; script.len()];
        let mut next_cue = 0usize;
        let path = source_wav_path();
        let (raw, channels, native_rate) = crate::audio::decode_audio(&path).expect("decode");
        let samples = if native_rate == SAMPLE_RATE {
            raw
        } else {
            crate::audio::resample_linear(&raw, channels, native_rate, SAMPLE_RATE)
        };
        let samples = Arc::new(samples);
        let total_frames = samples.len() / channels;

        let mut channel_pairs: Vec<(&str, DeckState, ChannelStrip)> = crate::audio::LIVE_DECK_IDS
            .iter()
            .map(|&id| {
                let mut deck = DeckState::empty(SAMPLE_RATE);
                deck.samples = Arc::clone(&samples);
                deck.channels = channels;
                deck.device_sample_rate = SAMPLE_RATE;
                deck.total_frames = total_frames;
                deck.duration = total_frames as f64 / SAMPLE_RATE as f64;
                let strip = ChannelStrip::from_manifest(
                    &session_core::CLASSIC_3BAND_V2,
                    SAMPLE_RATE as f32,
                );
                (id, deck, strip)
            })
            .collect();

        let mut limiter = LimiterState::new(SAMPLE_RATE as f32);
        let mut out = Vec::with_capacity(TOTAL * 2);
        let mut block_buffer: Vec<f32> = Vec::new();

        let mut frame = 0usize;
        let mut index = 0usize;
        while frame < TOTAL {
            let size = sizes[index % sizes.len()].min(TOTAL - frame);
            while next_cue < script.len() && frame >= script[next_cue].0 {
                let (_, id, cue) = script[next_cue];
                let (_, deck, _) = channel_pairs
                    .iter_mut()
                    .find(|(pair_id, _, _)| *pair_id == id)
                    .expect("scripted deck");
                match cue {
                    Cue::Play => deck.is_playing = true,
                    Cue::Stop => deck.is_playing = false,
                    Cue::Jog => deck.jog_pending += TICKS,
                }
                fired[next_cue] = frame;
                next_cue += 1;
            }
            block_buffer.clear();
            block_buffer.resize(size * 2, 0.0);
            for (_, deck, strip) in channel_pairs.iter_mut() {
                deck.render_block(
                    strip,
                    size,
                    RenderTargets {
                        main: Some(&mut block_buffer),
                        cue: None,
                    },
                );
            }
            for frame_index in 0..size {
                let (l, r) = crate::audio::master_output(
                    Some(&mut limiter),
                    block_buffer[frame_index * 2] * crate::audio::DEFAULT_MASTER_GAIN,
                    block_buffer[frame_index * 2 + 1] * crate::audio::DEFAULT_MASTER_GAIN,
                );
                out.push(l);
                out.push(r);
            }
            frame += size;
            index += 1;
        }
        (out, fired)
    }

    const TOTAL: usize = BLOCKS * BLOCK;

    fn offline_render(script: &[(usize, &str, Cue)], fired: &[usize]) -> Vec<f32> {
        let mut events = vec![format!(
            r#"{{"elapsed_ms":0,"frame":0,"type":"recording_start","buffer_size_frames":{BLOCK}}}"#
        )];
        for id in crate::audio::LIVE_DECK_IDS {
            events.push(format!(
                r#"{{"elapsed_ms":0,"frame":0,"type":"load_track","deck":"{id}","path":"__SOURCE__"}}"#
            ));
        }
        for ((_, id, cue), frame) in script.iter().zip(fired) {
            events.push(match cue {
                Cue::Play => {
                    format!(r#"{{"elapsed_ms":1,"frame":{frame},"type":"play","deck":"{id}"}}"#)
                }
                Cue::Stop => {
                    format!(r#"{{"elapsed_ms":1,"frame":{frame},"type":"stop","deck":"{id}"}}"#)
                }
                Cue::Jog => format!(
                    r#"{{"elapsed_ms":1,"frame":{frame},"type":"jog","deck":"{id}","ticks":{TICKS}}}"#
                ),
            });
        }
        let json = format!(
            r#"{{"version":2,"mixer":{header},"events":[{events}]}}"#,
            header =
                serde_json::to_string(&session_core::CLASSIC_3BAND_V2.header()).expect("header"),
            events = events.join(",\n")
        )
        .replace("__SOURCE__", &source_wav_path());
        let session: SessionFile = serde_json::from_str(&json).expect("parse session");
        render_session(
            &session,
            RenderRequest {
                sample_rate: SAMPLE_RATE,
                min_frames: TOTAL,
                limiter: MasterLimiter::On,
            },
        )
        .expect("render")
    }

    fn assert_parity(script: &[(usize, &str, Cue)]) {
        for sizes in SCHEDULES {
            let (live, fired) = live_render(sizes, script);
            let offline = offline_render(script, &fired);
            assert_eq!(live, offline[..live.len()], "block schedule {sizes:?}");
        }
    }

    #[test]
    fn the_offline_render_matches_the_live_block_path_at_any_block_length() {
        assert_parity(&[(PLAY_FRAME, "A", Cue::Play), (JOG_FRAME, "A", Cue::Jog)]);
    }

    #[test]
    fn a_resume_after_the_strip_settles_matches_the_live_block_path_at_any_block_length() {
        assert_parity(&[
            (PLAY_FRAME, "A", Cue::Play),
            (STOP_FRAME, "A", Cue::Stop),
            (RESUME_FRAME, "A", Cue::Play),
        ]);
    }

    #[test]
    fn a_scrub_before_play_matches_the_live_block_path_at_any_block_length() {
        assert_parity(&[
            (SCRUB_FRAME, "A", Cue::Jog),
            (SCRUB_PLAY_FRAME, "A", Cue::Play),
        ]);
    }

    #[test]
    fn two_decks_summed_and_limited_match_the_live_block_path_at_any_block_length() {
        assert_parity(&[
            (PLAY_FRAME, "A", Cue::Play),
            (JOG_FRAME, "B", Cue::Play),
            (SCRUB_PLAY_FRAME, "A", Cue::Jog),
            (STOP_FRAME + BLOCK, "B", Cue::Jog),
            (RESUME_FRAME, "A", Cue::Stop),
        ]);
    }
}
