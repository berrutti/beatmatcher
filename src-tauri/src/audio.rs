use std::sync::{Arc, Mutex};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Serialize;

// ── Public types returned to the frontend ────────────────────────────────────

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfo {
    pub duration: f64,
    pub sample_rate: u32,
    pub bpm: Option<f64>,
    pub silence_end: f64,
}

#[derive(Serialize, Clone)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
}

// ── Biquad filter (Direct Form II Transposed) ─────────────────────────────────
//
// Coefficients follow the Audio EQ Cookbook (Robert Bristow-Johnson).

#[derive(Copy, Clone)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    s1: f32,
    s2: f32,
}

impl Biquad {
    fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            s1: 0.0,
            s2: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.s1;
        self.s1 = self.b1 * x - self.a1 * y + self.s2;
        self.s2 = self.b2 * x - self.a2 * y;
        y
    }

    fn low_shelf(sr: f32, freq: f32, db: f32) -> Self {
        if db == 0.0 {
            return Self::identity();
        }
        let a = 10.0f32.powf(db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq / sr;
        let cos_w = w0.cos();
        // S = 1 (unity shelf slope) → alpha = sin(w0) / sqrt(2)
        let alpha = w0.sin() / 2.0_f32.sqrt();
        let k = 2.0 * a.sqrt() * alpha;

        let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w + k);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w);
        let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w - k);
        let a0 = (a + 1.0) + (a - 1.0) * cos_w + k;
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w);
        let a2 = (a + 1.0) + (a - 1.0) * cos_w - k;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            s1: 0.0,
            s2: 0.0,
        }
    }

    fn peaking(sr: f32, freq: f32, q: f32, db: f32) -> Self {
        if db == 0.0 {
            return Self::identity();
        }
        let a = 10.0f32.powf(db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq / sr;
        let alpha = w0.sin() / (2.0 * q);
        let cos_w = w0.cos();

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w;
        let a2 = 1.0 - alpha / a;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            s1: 0.0,
            s2: 0.0,
        }
    }

    fn high_shelf(sr: f32, freq: f32, db: f32) -> Self {
        if db == 0.0 {
            return Self::identity();
        }
        let a = 10.0f32.powf(db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq / sr;
        let cos_w = w0.cos();
        let alpha = w0.sin() / 2.0_f32.sqrt();
        let k = 2.0 * a.sqrt() * alpha;

        let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w + k);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w);
        let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w - k);
        let a0 = (a + 1.0) - (a - 1.0) * cos_w + k;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w);
        let a2 = (a + 1.0) - (a - 1.0) * cos_w - k;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            s1: 0.0,
            s2: 0.0,
        }
    }
}

// ── 3-band EQ (2 channels) ────────────────────────────────────────────────────

struct EqState {
    sample_rate: f32,
    low: [Biquad; 2],
    mid: [Biquad; 2],
    high: [Biquad; 2],
}

impl EqState {
    fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            low: [Biquad::identity(), Biquad::identity()],
            mid: [Biquad::identity(), Biquad::identity()],
            high: [Biquad::identity(), Biquad::identity()],
        }
    }

    fn set_low(&mut self, db: f32) {
        let new_filter = Biquad::low_shelf(self.sample_rate, 100.0, db);
        for channel in &mut self.low {
            let (s1, s2) = (channel.s1, channel.s2);
            *channel = new_filter;
            channel.s1 = s1;
            channel.s2 = s2;
        }
    }

    fn set_mid(&mut self, db: f32) {
        let new_filter = Biquad::peaking(self.sample_rate, 1000.0, 1.0, db);
        for channel in &mut self.mid {
            let (s1, s2) = (channel.s1, channel.s2);
            *channel = new_filter;
            channel.s1 = s1;
            channel.s2 = s2;
        }
    }

    fn set_high(&mut self, db: f32) {
        let new_filter = Biquad::high_shelf(self.sample_rate, 8000.0, db);
        for channel in &mut self.high {
            let (s1, s2) = (channel.s1, channel.s2);
            *channel = new_filter;
            channel.s1 = s1;
            channel.s2 = s2;
        }
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let l = self.high[0].process(self.mid[0].process(self.low[0].process(l)));
        let r = self.high[1].process(self.mid[1].process(self.low[1].process(r)));
        (l.clamp(-1.0, 1.0), r.clamp(-1.0, 1.0))
    }
}

// ── Deck state ─────────────────────────────────────────────────────────────────
//
// Two positions are tracked independently:
//   main_pos: advanced by the main output stream callback (source of truth)
//   cue_pos:  advanced by the cue output stream callback
//
// Both start from the same point on play() and advance at the same rate, so
// they stay in sync. Minor drift (sub-ms) is imperceptible for monitoring.

pub struct DeckState {
    pub samples: Arc<Vec<f32>>, // interleaved f32 at device_sample_rate
    pub channels: usize,
    pub device_sample_rate: u32,
    pub total_frames: usize,
    pub duration: f64,

    pub is_playing: bool,
    pub main_pos: f64, // fractional frame index
    pub cue_pos: f64,  // fractional frame index (independent of main_pos)
    pub loop_active: bool,
    pub loop_start: f64, // in frames
    pub loop_end: f64,   // in frames
    pub playback_rate: f64,
    pub nudge_factor: f64, // 1 + nudge_percent/100
    pub gain: f32,
    pub cue_active: bool,

    eq: EqState,
}

impl DeckState {
    pub fn empty(device_sample_rate: u32) -> Self {
        Self {
            samples: Arc::new(Vec::new()),
            channels: 2,
            device_sample_rate,
            total_frames: 0,
            duration: 0.0,
            is_playing: false,
            main_pos: 0.0,
            cue_pos: 0.0,
            loop_active: false,
            loop_start: 0.0,
            loop_end: 0.0,
            playback_rate: 1.0,
            nudge_factor: 1.0,
            gain: 1.0,
            cue_active: false,
            eq: EqState::new(device_sample_rate as f32),
        }
    }

    pub fn position_sec(&self) -> f64 {
        if self.device_sample_rate == 0 {
            return 0.0;
        }
        self.main_pos / self.device_sample_rate as f64
    }

    pub fn set_eq_band(&mut self, band: &str, db: f32) {
        match band {
            "low" => self.eq.set_low(db),
            "mid" => self.eq.set_mid(db),
            "high" => self.eq.set_high(db),
            _ => {}
        }
    }

    // Called by the main output stream callback. Advances main_pos and applies EQ.
    #[inline]
    pub fn main_tick(&mut self) -> (f32, f32) {
        if !self.is_playing || self.samples.is_empty() {
            return (0.0, 0.0);
        }
        let (l, r) = self.read_at(self.main_pos);
        let (l, r) = self.eq.process(l, r);
        let l = l * self.gain;
        let r = r * self.gain;
        self.main_pos = self.next_pos(self.main_pos, true);
        (l, r)
    }

    // Called by the cue output stream callback. Advances cue_pos without EQ.
    #[inline]
    pub fn cue_tick(&mut self) -> (f32, f32) {
        if !self.is_playing || !self.cue_active || self.samples.is_empty() {
            return (0.0, 0.0);
        }
        let (l, r) = self.read_at(self.cue_pos);
        let l = l * self.gain;
        let r = r * self.gain;
        self.cue_pos = self.next_pos(self.cue_pos, false);
        (l, r)
    }

    fn read_at(&self, pos: f64) -> (f32, f32) {
        let frame_index = pos as usize;
        let interp_factor = (pos - frame_index as f64) as f32;

        let lo_frame = frame_index.min(self.total_frames.saturating_sub(1));
        let hi_frame = (frame_index + 1).min(self.total_frames.saturating_sub(1));

        if self.channels == 1 {
            let lo_sample = self.samples[lo_frame];
            let hi_sample = self.samples[hi_frame];
            let s = lo_sample + interp_factor * (hi_sample - lo_sample);
            (s, s)
        } else {
            let lo_idx = lo_frame * self.channels;
            let hi_idx = hi_frame * self.channels;
            let l = self.samples[lo_idx] + interp_factor * (self.samples[hi_idx] - self.samples[lo_idx]);
            let r = self.samples[lo_idx + 1] + interp_factor * (self.samples[hi_idx + 1] - self.samples[lo_idx + 1]);
            (l, r)
        }
    }

    fn next_pos(&mut self, pos: f64, is_main: bool) -> f64 {
        let step = self.playback_rate * self.nudge_factor;
        let new_pos = pos + step;

        if self.loop_active && new_pos >= self.loop_end {
            let dur = self.loop_end - self.loop_start;
            return if dur > 0.0 {
                self.loop_start + (new_pos - self.loop_end) % dur
            } else {
                self.loop_start
            };
        }

        if new_pos >= self.total_frames as f64 {
            if is_main {
                self.is_playing = false;
            }
            return self.total_frames as f64;
        }

        new_pos
    }
}

// ── cpal stream (Send wrapper) ────────────────────────────────────────────────
//
// cpal::Stream is !Send on some platforms (CoreAudio on macOS holds raw pointers).
// We wrap it to allow storage in Tauri managed state. The stream is never moved
// to another thread after creation; it's only kept alive by being held in AppAudio.

#[allow(dead_code)]
struct SendStream(cpal::Stream);
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

// ── Audio engine ───────────────────────────────────────────────────────────────

pub struct AppAudio {
    pub deck_a: Arc<Mutex<DeckState>>,
    pub deck_b: Arc<Mutex<DeckState>>,
    pub device_sample_rate: u32,
    pub device_channels: usize,
    _main_stream: SendStream,
    _cue_stream: Mutex<Option<SendStream>>,
}

// AppAudio is held in Tauri managed state and accessed from async command threads.
unsafe impl Send for AppAudio {}
unsafe impl Sync for AppAudio {}

impl AppAudio {
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no default output device")?;
        let config = device.default_output_config()?;
        let device_sample_rate = config.sample_rate().0;
        let device_channels = config.channels() as usize;

        let deck_a = Arc::new(Mutex::new(DeckState::empty(device_sample_rate)));
        let deck_b = Arc::new(Mutex::new(DeckState::empty(device_sample_rate)));

        let main_stream = build_stream(
            &device,
            &config,
            Arc::clone(&deck_a),
            Arc::clone(&deck_b),
            device_channels,
            false,
        )?;
        main_stream.play()?;

        Ok(Self {
            deck_a,
            deck_b,
            device_sample_rate,
            device_channels,
            _main_stream: SendStream(main_stream),
            _cue_stream: Mutex::new(None),
        })
    }

    pub fn deck(&self, id: &str) -> Option<Arc<Mutex<DeckState>>> {
        match id {
            "A" => Some(Arc::clone(&self.deck_a)),
            "B" => Some(Arc::clone(&self.deck_b)),
            _ => None,
        }
    }

    pub fn list_devices(&self) -> Vec<DeviceInfo> {
        let host = cpal::default_host();
        host.output_devices()
            .map(|devices| {
                devices
                    .filter_map(|d| {
                        let name = d.name().ok()?;
                        Some(DeviceInfo {
                            id: name.clone(),
                            name,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn set_cue_device(&self, device_id: &str) -> Result<(), String> {
        let host = cpal::default_host();
        let device = host
            .output_devices()
            .map_err(|e| e.to_string())?
            .find(|d| d.name().map(|n| n == device_id).unwrap_or(false))
            .ok_or_else(|| format!("device not found: {}", device_id))?;

        let config = device
            .default_output_config()
            .map_err(|e| e.to_string())?;

        let channels = config.channels() as usize;
        let stream = build_stream(
            &device,
            &config,
            Arc::clone(&self.deck_a),
            Arc::clone(&self.deck_b),
            channels,
            true,
        )
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;

        *self._cue_stream.lock().unwrap() = Some(SendStream(stream));
        Ok(())
    }
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    deck_a: Arc<Mutex<DeckState>>,
    deck_b: Arc<Mutex<DeckState>>,
    output_channels: usize,
    is_cue: bool,
) -> Result<cpal::Stream, Box<dyn std::error::Error + Send + Sync>> {
    let stream_config: cpal::StreamConfig = config.clone().into();

    match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let stream = device.build_output_stream(
                &stream_config,
                move |data: &mut [f32], _| {
                    fill_output(data, output_channels, &deck_a, &deck_b, is_cue);
                },
                |e| eprintln!("audio stream error: {:?}", e),
                None,
            )?;
            Ok(stream)
        }
        cpal::SampleFormat::I16 => {
            let stream = device.build_output_stream(
                &stream_config,
                move |data: &mut [i16], _| {
                    let mut buf = vec![0.0f32; data.len()];
                    fill_output(&mut buf, output_channels, &deck_a, &deck_b, is_cue);
                    for (d, s) in data.iter_mut().zip(buf.iter()) {
                        *d = (*s * i16::MAX as f32) as i16;
                    }
                },
                |e| eprintln!("audio stream error: {:?}", e),
                None,
            )?;
            Ok(stream)
        }
        fmt => Err(format!("unsupported sample format: {:?}", fmt).into()),
    }
}

fn fill_output(
    data: &mut [f32],
    output_channels: usize,
    deck_a: &Arc<Mutex<DeckState>>,
    deck_b: &Arc<Mutex<DeckState>>,
    is_cue: bool,
) {
    data.fill(0.0);
    let frames = data.len() / output_channels.max(1);

    {
        let mut deck = deck_a.lock().unwrap();
        for i in 0..frames {
            let (l, r) = if is_cue { deck.cue_tick() } else { deck.main_tick() };
            mix_frame(data, i, output_channels, l, r);
        }
    }
    {
        let mut deck = deck_b.lock().unwrap();
        for i in 0..frames {
            let (l, r) = if is_cue { deck.cue_tick() } else { deck.main_tick() };
            mix_frame(data, i, output_channels, l, r);
        }
    }

    for sample in data.iter_mut() {
        *sample = sample.clamp(-1.0, 1.0);
    }
}

#[inline]
fn mix_frame(data: &mut [f32], frame: usize, channels: usize, l: f32, r: f32) {
    if channels == 1 {
        data[frame] += (l + r) * 0.5;
    } else {
        data[frame * channels] += l;
        data[frame * channels + 1] += r;
    }
}

// ── Audio decoding ─────────────────────────────────────────────────────────────

pub fn decode_audio(
    path: &str,
) -> Result<(Vec<f32>, usize, u32), Box<dyn std::error::Error + Send + Sync>> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
    {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("no audio track found")?;

    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    let sample_rate = codec_params.sample_rate.unwrap_or(44100);

    let mut decoder =
        symphonia::default::get_codecs().make(&codec_params, &DecoderOptions::default())?;

    let capacity = codec_params.n_frames.map(|n| n as usize * 2).unwrap_or(0);
    let mut samples: Vec<f32> = Vec::with_capacity(capacity);
    // Determined from the first decoded packet spec rather than codec_params,
    // because codec_params.channels can be None for some formats even when the
    // audio is mono, which would cause wrong chunk-size when mixing to mono.
    let mut actual_channels: Option<usize> = None;

    log::info!("decode_audio: opening '{}', native_sr={}", path, sample_rate);

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(_)) => break,
            Err(symphonia::core::errors::Error::ResetRequired) => continue,
            Err(e) => return Err(e.into()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                if decoded.frames() == 0 {
                    continue;
                }
                // Capture spec and channel count before consuming decoded
                let spec = *decoded.spec();
                let src_channels = spec.channels.count();
                // Lock in the channel count from the first packet.
                let out_channels = *actual_channels.get_or_insert_with(|| {
                    let ch = src_channels.min(2);
                    log::info!("decode_audio: first packet spec src_channels={} out_channels={}", src_channels, ch);
                    ch
                });
                let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buf.copy_interleaved_ref(decoded);
                let src = buf.samples();
                if src_channels <= 2 {
                    samples.extend_from_slice(src);
                } else {
                    // More than stereo: keep only L and R
                    for frame in src.chunks(src_channels) {
                        samples.push(frame[0]);
                        if out_channels > 1 {
                            samples.push(frame[1]);
                        }
                    }
                }
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        }
    }

    let channels = actual_channels.unwrap_or(1);
    let total_frames = samples.len() / channels.max(1);
    log::info!(
        "decode_audio: done, channels={}, frames={}, duration={:.2}s",
        channels,
        total_frames,
        total_frames as f64 / sample_rate as f64
    );

    Ok((samples, channels, sample_rate))
}

// ── Resampling (linear interpolation) ────────────────────────────────────────

pub fn resample_linear(
    input: &[f32],
    in_channels: usize,
    in_rate: u32,
    out_rate: u32,
) -> Vec<f32> {
    let in_frames = input.len() / in_channels;
    let ratio = in_rate as f64 / out_rate as f64;
    let out_frames = (in_frames as f64 / ratio).ceil() as usize;
    let mut output = Vec::with_capacity(out_frames * in_channels);

    for out_frame in 0..out_frames {
        let src_pos = out_frame as f64 * ratio;
        let src_frame = src_pos as usize;
        let interp_factor = (src_pos - src_frame as f64) as f32;
        let lo_frame = src_frame.min(in_frames.saturating_sub(1));
        let hi_frame = (src_frame + 1).min(in_frames.saturating_sub(1));

        for ch in 0..in_channels {
            let lo_sample = input[lo_frame * in_channels + ch];
            let hi_sample = input[hi_frame * in_channels + ch];
            output.push(lo_sample + interp_factor * (hi_sample - lo_sample));
        }
    }

    output
}

// ── Waveform peak extraction ──────────────────────────────────────────────────

/// Compute `num_points` peak amplitude values for the region [start_sec, end_sec]
/// of the (resampled, interleaved) sample buffer at `device_sample_rate`.
/// Always returns exactly `num_points` values. Resolution adapts to the zoom
/// level: each point covers (end_sec - start_sec) / num_points seconds of audio,
/// giving pixel-perfect detail at any zoom without pre-baked resolution limits.
pub fn compute_waveform_region(
    samples: &[f32],
    channels: usize,
    device_sample_rate: u32,
    start_sec: f64,
    end_sec: f64,
    num_points: usize,
) -> Vec<f32> {
    if samples.is_empty() || channels == 0 || num_points == 0 {
        return vec![0.0; num_points];
    }
    let total_frames = samples.len() / channels;
    let sample_rate = device_sample_rate as f64;
    let start_frame = (start_sec * sample_rate).max(0.0) as usize;
    let end_frame = ((end_sec * sample_rate) as usize).min(total_frames);

    if start_frame >= end_frame {
        return vec![0.0; num_points];
    }

    let visible_frames = end_frame - start_frame;
    let frames_per_point = visible_frames as f64 / num_points as f64;

    (0..num_points)
        .map(|point_index| {
            let bin_start = start_frame + (point_index as f64 * frames_per_point) as usize;
            let bin_end = (start_frame + ((point_index + 1) as f64 * frames_per_point) as usize)
                .min(end_frame)
                .max(bin_start + 1);
            let mut max_amplitude = 0.0f32;
            for frame in bin_start..bin_end {
                for ch in 0..channels {
                    let amplitude = samples[frame * channels + ch].abs();
                    if amplitude > max_amplitude {
                        max_amplitude = amplitude;
                    }
                }
            }
            max_amplitude
        })
        .collect()
}

// ── BPM detection ─────────────────────────────────────────────────────────────
//
// Algorithm mirrors the existing bpmDetect.ts logic: lowpass filter, peak
// finding, interval counting, BPM clustering.

const BPM_MIN: f64 = 90.0;
const BPM_MAX: f64 = 180.0;
const PEAK_SKIP_SAMPLES: usize = 10_000;
const NEIGHBOR_COUNT: usize = 10;
const CLUSTER_TOLERANCE: f64 = 1.0;
const THRESHOLDS: &[f32] = &[0.9, 0.8, 0.7];
const MIN_PEAKS: usize = 15;

// 2nd-order Butterworth lowpass matching Web Audio BiquadFilterNode (type='lowpass', default Q=1/sqrt(2)).
fn lowpass_biquad(input: &[f32], sample_rate: u32, cutoff_hz: f32) -> Vec<f32> {
    use std::f64::consts::PI;
    let w0 = 2.0 * PI * cutoff_hz as f64 / sample_rate as f64;
    let cos_w0 = w0.cos();
    // alpha = sin(w0) / (2*Q), Q = 1/sqrt(2)  =>  alpha = sin(w0) / sqrt(2)
    let alpha = w0.sin() / std::f64::consts::SQRT_2;
    let b0 = (1.0 - cos_w0) / 2.0;
    let b1 = 1.0 - cos_w0;
    let b2 = (1.0 - cos_w0) / 2.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha;
    let mut output = Vec::with_capacity(input.len());
    let mut x1 = 0.0f64;
    let mut x2 = 0.0f64;
    let mut y1 = 0.0f64;
    let mut y2 = 0.0f64;
    for &in_sample in input {
        let x0 = in_sample as f64;
        let y = (b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2) / a0;
        output.push(y as f32);
        x2 = x1;
        x1 = x0;
        y2 = y1;
        y1 = y;
    }
    output
}

fn find_peaks(data: &[f32], threshold: f32, skip: usize) -> Vec<usize> {
    let mut peaks = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if data[i].abs() > threshold {
            peaks.push(i);
            i += skip;
        }
        i += 1;
    }
    peaks
}

fn interval_to_bpm(interval: usize, sample_rate: u32) -> Option<f64> {
    if interval == 0 {
        return None;
    }
    let mut bpm = 60.0 * sample_rate as f64 / interval as f64;
    while bpm < BPM_MIN { bpm *= 2.0; }
    while bpm > BPM_MAX { bpm /= 2.0; }
    if bpm >= BPM_MIN && bpm <= BPM_MAX { Some(bpm) } else { None }
}

struct BpmCluster {
    weighted_bpm_sum: f64,
    count: usize,
}

pub fn detect_bpm(mono: &[f32], sample_rate: u32) -> Option<f64> {
    let filtered = lowpass_biquad(mono, sample_rate, 150.0);

    let mut peaks = Vec::new();
    for &threshold in THRESHOLDS {
        peaks = find_peaks(&filtered, threshold, PEAK_SKIP_SAMPLES);
        if peaks.len() >= MIN_PEAKS {
            break;
        }
    }

    if peaks.len() < 2 {
        return None;
    }

    let mut interval_counts: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for i in 0..peaks.len() {
        let limit = (i + NEIGHBOR_COUNT + 1).min(peaks.len());
        for j in (i + 1)..limit {
            let interval = peaks[j] - peaks[i];
            *interval_counts.entry(interval).or_insert(0) += 1;
        }
    }

    let mut clusters: Vec<BpmCluster> = Vec::new();

    for (&interval, &count) in &interval_counts {
        if let Some(bpm) = interval_to_bpm(interval, sample_rate) {
            let mut merged = false;
            for cluster in &mut clusters {
                let cluster_avg = cluster.weighted_bpm_sum / cluster.count as f64;
                if (cluster_avg - bpm).abs() <= CLUSTER_TOLERANCE {
                    cluster.weighted_bpm_sum += bpm * count as f64;
                    cluster.count += count;
                    merged = true;
                    break;
                }
            }
            if !merged {
                clusters.push(BpmCluster { weighted_bpm_sum: bpm * count as f64, count });
            }
        }
    }

    clusters.sort_by(|a, b| b.count.cmp(&a.count));
    let result = clusters.first().map(|cluster| {
        let bpm = cluster.weighted_bpm_sum / cluster.count as f64;
        (bpm * 10.0).round() / 10.0
    });
    log::info!(
        "detect_bpm: peaks={} clusters={} result={:?}",
        peaks.len(),
        clusters.len(),
        result
    );
    result
}

pub fn detect_silence_end(mono: &[f32], sample_rate: u32) -> f64 {
    const THRESHOLD: f32 = 0.01;
    let window_frames = (sample_rate as usize / 20).max(1); // 50ms = sample_rate / 20

    let mut frame = 0;
    while frame + window_frames <= mono.len() {
        let rms = (mono[frame..frame + window_frames]
            .iter()
            .map(|&x| x * x)
            .sum::<f32>()
            / window_frames as f32)
            .sqrt();

        if rms > THRESHOLD {
            let silence_end_secs = frame as f64 / sample_rate as f64;
            log::info!(
                "detect_silence_end: audio starts at {:.3}s (frame {}, rms={:.5})",
                silence_end_secs,
                frame,
                rms
            );
            return silence_end_secs;
        }

        frame += window_frames;
    }

    log::info!(
        "detect_silence_end: no audio above threshold {:.4} found in {} samples, returning 0.0",
        THRESHOLD,
        mono.len()
    );
    0.0
}
