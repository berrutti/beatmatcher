use std::collections::HashMap;
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
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub channels: usize,
}

// ── Biquad filter (Direct Form II Transposed) ─────────────────────────────────
//
// Implements H(z) = (b0 + b1·z⁻¹ + b2·z⁻²) / (1 + a1·z⁻¹ + a2·z⁻²)
// Coefficients follow the Audio EQ Cookbook (Robert Bristow-Johnson).
//
// b0/b1/b2  — feedforward: how much of the current and past INPUT samples
//             contribute to the output. Together they shape the frequency
//             response (e.g. low-pass attenuates high-frequency input).
// a1/a2     — feedback: how much of the past OUTPUT samples feed back into
//             the filter. This creates resonance/poles in the response.
// delay1/2  — the two internal memory cells that carry state between samples.

#[derive(Copy, Clone)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    delay1: f32,
    delay2: f32,
}

impl Biquad {
    fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            delay1: 0.0,
            delay2: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.delay1;
        self.delay1 = self.b1 * x - self.a1 * y + self.delay2;
        self.delay2 = self.b2 * x - self.a2 * y;
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
            delay1: 0.0,
            delay2: 0.0,
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
            delay1: 0.0,
            delay2: 0.0,
        }
    }

    fn low_pass(sr: f32, freq: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * freq / sr;
        let cos_w = w0.cos();
        let alpha = w0.sin() / (2.0 * q);

        let b0 = (1.0 - cos_w) / 2.0;
        let b1 = 1.0 - cos_w;
        let b2 = (1.0 - cos_w) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w;
        let a2 = 1.0 - alpha;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            delay1: 0.0,
            delay2: 0.0,
        }
    }

    fn high_pass(sr: f32, freq: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * freq / sr;
        let cos_w = w0.cos();
        let alpha = w0.sin() / (2.0 * q);

        let b0 = (1.0 + cos_w) / 2.0;
        let b1 = -(1.0 + cos_w);
        let b2 = (1.0 + cos_w) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w;
        let a2 = 1.0 - alpha;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            delay1: 0.0,
            delay2: 0.0,
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
            delay1: 0.0,
            delay2: 0.0,
        }
    }
}

// ── 3-band EQ (2 channels) ────────────────────────────────────────────────────
//
// Low and high use two cascaded 2nd-order shelves (= 4th-order total) so the
// rolloff is steep enough to decisively kill bass and treble. Mid uses a single
// wide-Q peaking filter to cover the full vocal/instrument range.

const EQ_LOW_SHELF_HZ: f32 = 70.0;
const EQ_MID_PEAK_HZ: f32 = 1000.0;
const EQ_MID_Q: f32 = 0.5;
const EQ_HIGH_SHELF_HZ: f32 = 13_000.0;

struct EqState {
    sample_rate: f32,
    low_stage1:  [Biquad; 2],
    low_stage2:  [Biquad; 2],
    mid:         [Biquad; 2],
    high_stage1: [Biquad; 2],
    high_stage2: [Biquad; 2],
}

impl EqState {
    fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            low_stage1:  [Biquad::identity(), Biquad::identity()],
            low_stage2:  [Biquad::identity(), Biquad::identity()],
            mid:         [Biquad::identity(), Biquad::identity()],
            high_stage1: [Biquad::identity(), Biquad::identity()],
            high_stage2: [Biquad::identity(), Biquad::identity()],
        }
    }

    fn set_low(&mut self, db: f32) {
        let stage = Biquad::low_shelf(self.sample_rate, EQ_LOW_SHELF_HZ, db / 2.0);
        for (first, second) in self.low_stage1.iter_mut().zip(self.low_stage2.iter_mut()) {
            let (prev_delay1, prev_delay2) = (first.delay1, first.delay2);
            *first = stage;
            first.delay1 = prev_delay1;
            first.delay2 = prev_delay2;
            let (prev_delay1, prev_delay2) = (second.delay1, second.delay2);
            *second = stage;
            second.delay1 = prev_delay1;
            second.delay2 = prev_delay2;
        }
    }

    fn set_mid(&mut self, db: f32) {
        let filter = Biquad::peaking(self.sample_rate, EQ_MID_PEAK_HZ, EQ_MID_Q, db);
        for ch in &mut self.mid {
            let (prev_delay1, prev_delay2) = (ch.delay1, ch.delay2);
            *ch = filter;
            ch.delay1 = prev_delay1;
            ch.delay2 = prev_delay2;
        }
    }

    fn set_high(&mut self, db: f32) {
        let stage = Biquad::high_shelf(self.sample_rate, EQ_HIGH_SHELF_HZ, db / 2.0);
        for (first, second) in self.high_stage1.iter_mut().zip(self.high_stage2.iter_mut()) {
            let (prev_delay1, prev_delay2) = (first.delay1, first.delay2);
            *first = stage;
            first.delay1 = prev_delay1;
            first.delay2 = prev_delay2;
            let (prev_delay1, prev_delay2) = (second.delay1, second.delay2);
            *second = stage;
            second.delay1 = prev_delay1;
            second.delay2 = prev_delay2;
        }
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let l = self.low_stage2[0].process(self.low_stage1[0].process(l));
        let r = self.low_stage2[1].process(self.low_stage1[1].process(r));
        let l = self.mid[0].process(l);
        let r = self.mid[1].process(r);
        let l = self.high_stage2[0].process(self.high_stage1[0].process(l));
        let r = self.high_stage2[1].process(self.high_stage1[1].process(r));
        (l, r)
    }
}

// HPF/LPF filter
//   v = 0:  bypass (small dead zone around center)
//   v < 0:  LPF, cutoff sweeps from 20 kHz down to 20 Hz as v -> -1
//   v > 0:  HPF, cutoff sweeps from 20 Hz up to 20 kHz as v -> +1
//
// Serial signal path.
// Fixed Q for clean, musical filtering.
// Current_v is smoothed per-sample; coefficients are refreshed every
// FILTER_COEF_REFRESH samples to keep the DSP inner loop tight.

const FILTER_MIN_FREQ_HZ: f32 = 20.0;
const FILTER_MAX_FREQ_HZ: f32 = 20_000.0;
const FILTER_RESONANCE_Q: f32 = 2.0;
const FILTER_CENTER_DEAD_ZONE: f32 = 0.05;
const FILTER_SMOOTHING_TAU_SEC: f32 = 0.015;
const FILTER_COEFF_REFRESH_INTERVAL: u32 = 16;

struct FilterState {
    sample_rate: f32,
    target_knob: f32,
    current_knob: f32,
    smoothing_coeff: f32,
    coeff_refresh_counter: u32,
    filters: [Biquad; 2],
}

impl FilterState {
    fn new(sample_rate: f32) -> Self {
        let smoothing_coeff = 1.0 - (-1.0 / (sample_rate * FILTER_SMOOTHING_TAU_SEC)).exp();
        Self {
            sample_rate,
            target_knob: 0.0,
            current_knob: 0.0,
            smoothing_coeff,
            coeff_refresh_counter: 0,
            filters: [Biquad::identity(), Biquad::identity()],
        }
    }

    fn set(&mut self, v: f32) {
        self.target_knob = v.clamp(-1.0, 1.0);
    }

    fn update_filters(&mut self) {
        let knob = self.current_knob;
        let abs_knob = knob.abs();

        let new_filter = if abs_knob <= FILTER_CENTER_DEAD_ZONE {
            Biquad::identity()
        } else {
            let sweep = (abs_knob - FILTER_CENTER_DEAD_ZONE) / (1.0 - FILTER_CENTER_DEAD_ZONE);
            if knob < 0.0 {
                let cutoff = FILTER_MAX_FREQ_HZ * (FILTER_MIN_FREQ_HZ / FILTER_MAX_FREQ_HZ).powf(sweep);
                Biquad::low_pass(self.sample_rate, cutoff, FILTER_RESONANCE_Q)
            } else {
                let cutoff = FILTER_MIN_FREQ_HZ * (FILTER_MAX_FREQ_HZ / FILTER_MIN_FREQ_HZ).powf(sweep);
                Biquad::high_pass(self.sample_rate, cutoff, FILTER_RESONANCE_Q)
            }
        };

        for ch in &mut self.filters {
            let (prev_delay1, prev_delay2) = (ch.delay1, ch.delay2);
            *ch = new_filter;
            ch.delay1 = prev_delay1;
            ch.delay2 = prev_delay2;
        }
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.current_knob += (self.target_knob - self.current_knob) * self.smoothing_coeff;

        if self.coeff_refresh_counter == 0 {
            self.update_filters();
        }
        self.coeff_refresh_counter = (self.coeff_refresh_counter + 1) % FILTER_COEFF_REFRESH_INTERVAL;

        (self.filters[0].process(l), self.filters[1].process(r))
    }
}

// ── Channel strip ──────────────────────────────────────────────────────────────
// Mixer concerns: EQ, fader gain, and cue routing.

pub struct ChannelStrip {
    pub gain: f32,
    pub cue_active: bool,
    eq: EqState,
    eq_cue: EqState,
    filter: FilterState,
    filter_cue: FilterState,
}

impl ChannelStrip {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            gain: 1.0,
            cue_active: false,
            eq: EqState::new(sample_rate),
            eq_cue: EqState::new(sample_rate),
            filter: FilterState::new(sample_rate),
            filter_cue: FilterState::new(sample_rate),
        }
    }

    pub fn set_eq_band(&mut self, band: &str, db: f32) {
        match band {
            "low"  => { self.eq.set_low(db);  self.eq_cue.set_low(db); }
            "mid"  => { self.eq.set_mid(db);  self.eq_cue.set_mid(db); }
            "high" => { self.eq.set_high(db); self.eq_cue.set_high(db); }
            _ => {}
        }
    }

    pub fn set_filter(&mut self, v: f32) {
        self.filter.set(v);
        self.filter_cue.set(v);
    }

    // Applied to the master output path: EQ, filter, then fader gain.
    #[inline]
    pub fn process_main(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (l, r) = self.eq.process(l, r);
        let (l, r) = self.filter.process(l, r);
        (l * self.gain, r * self.gain)
    }

    // Applied to the cue output path: EQ then filter (pre-fader), gated by
    // cue_active. Always called so filter state stays in sync; output is
    // silenced when cue_active is false.
    #[inline]
    pub fn process_cue(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (l, r) = self.eq_cue.process(l, r);
        let (l, r) = self.filter_cue.process(l, r);
        if self.cue_active { (l, r) } else { (0.0, 0.0) }
    }
}

// ── Deck state ─────────────────────────────────────────────────────────────────
//
// Two positions are tracked independently:
//   main_pos: advanced by the master output stream callback (source of truth)
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
        }
    }

    pub fn position_sec(&self) -> f64 {
        if self.device_sample_rate == 0 {
            return 0.0;
        }
        self.main_pos / self.device_sample_rate as f64
    }

    // Reads the next master output sample and advances main_pos.
    #[inline]
    pub fn main_tick(&mut self) -> (f32, f32) {
        if !self.is_playing || self.samples.is_empty() {
            return (0.0, 0.0);
        }
        let (l, r) = self.read_at(self.main_pos);
        self.main_pos = self.next_pos(self.main_pos, true);
        (l, r)
    }

    // Reads the next cue sample and advances cue_pos. cue_pos always advances
    // while playing so it stays in sync with main_pos regardless of cue_active.
    #[inline]
    pub fn cue_tick(&mut self) -> (f32, f32) {
        if !self.is_playing || self.samples.is_empty() {
            return (0.0, 0.0);
        }
        let (l, r) = self.read_at(self.cue_pos);
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
    pub device_sample_rate: u32,
    decks: HashMap<String, Arc<Mutex<DeckState>>>,
    strips: HashMap<String, Arc<Mutex<ChannelStrip>>>,
    default_device_id: String,
    current_main_id: Mutex<String>,
    current_main_offset: Mutex<usize>,
    current_cue_id: Mutex<String>,   // empty string = no cue device configured
    current_cue_offset: Mutex<usize>,
    _main_stream: Mutex<SendStream>,
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
        let default_device_id = device.name().unwrap_or_default();
        let config = device.default_output_config()?;
        let device_sample_rate = config.sample_rate().0;

        let mut decks = HashMap::new();
        let mut strips = HashMap::new();
        for id in ["A", "B"] {
            decks.insert(id.to_string(), Arc::new(Mutex::new(DeckState::empty(device_sample_rate))));
            strips.insert(id.to_string(), Arc::new(Mutex::new(ChannelStrip::new(device_sample_rate as f32))));
        }

        let channels = channel_pairs(&decks, &strips);
        let main_stream = build_stream(&device, &config, channels, false, 0)?;
        main_stream.play()?;

        Ok(Self {
            device_sample_rate,
            decks,
            strips,
            current_main_id: Mutex::new(default_device_id.clone()),
            current_main_offset: Mutex::new(0),
            current_cue_id: Mutex::new(String::new()),
            current_cue_offset: Mutex::new(0),
            default_device_id,
            _main_stream: Mutex::new(SendStream(main_stream)),
            _cue_stream: Mutex::new(None),
        })
    }

    pub fn deck(&self, id: &str) -> Option<Arc<Mutex<DeckState>>> {
        self.decks.get(id).cloned()
    }

    pub fn strip(&self, id: &str) -> Option<Arc<Mutex<ChannelStrip>>> {
        self.strips.get(id).cloned()
    }

    pub fn list_devices(&self) -> Vec<DeviceInfo> {
        let host = cpal::default_host();
        // Use all devices (not just output_devices()) because output_devices()
        // filters by max_output_channels() > 0, which excludes inactive devices
        // (e.g. Bluetooth or USB audio not currently set as system output on macOS).
        // supported_output_configs() queries registered driver-level formats and
        // succeeds even for inactive devices, so we use that as the output check.
        host.devices()
            .map(|devices| {
                devices
                    .filter_map(|d| {
                        let name = d.name().ok()?;
                        let configs: Vec<_> = d.supported_output_configs().ok()?.collect();
                        if configs.is_empty() {
                            return None;
                        }
                        let max_channels = configs.iter().map(|c| c.channels() as usize).max().unwrap_or(2);
                        let is_default = name == self.default_device_id;
                        Some(DeviceInfo {
                            id: name.clone(),
                            name,
                            is_default,
                            channels: max_channels,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn set_cue_device(&self, device_id: &str, channel_offset: usize) -> Result<(), String> {
        log::info!("set_cue_device: id='{}' channel_offset={}", device_id, channel_offset);
        *self.current_cue_id.lock().unwrap() = device_id.to_string();
        *self.current_cue_offset.lock().unwrap() = channel_offset;
        self.rebuild_streams()
    }

    pub fn set_main_device(&self, device_id: &str, channel_offset: usize) -> Result<(), String> {
        let effective_id = if device_id.is_empty() { self.default_device_id.as_str() } else { device_id };
        log::info!("set_main_device: id='{}' channel_offset={}", effective_id, channel_offset);
        *self.current_main_id.lock().unwrap() = effective_id.to_string();
        *self.current_main_offset.lock().unwrap() = channel_offset;
        self.rebuild_streams()
    }

    // Inspect the current main/cue routing and build either a single combined
    // stream (when both are on the same device) or two separate streams.
    //
    // Using one combined callback when main and cue share a device prevents the
    // two separate CoreAudio render callbacks from interfering: a cue callback
    // that writes zeros doesn't blank out the main output buffer.
    fn rebuild_streams(&self) -> Result<(), String> {
        let main_id  = self.current_main_id.lock().unwrap().clone();
        let main_off = *self.current_main_offset.lock().unwrap();
        let cue_id   = self.current_cue_id.lock().unwrap().clone();
        let cue_off  = *self.current_cue_offset.lock().unwrap();

        log::info!("rebuild_streams: main='{}' off={} | cue='{}' off={}", main_id, main_off, cue_id, cue_off);

        let ch = channel_pairs(&self.decks, &self.strips);

        if !cue_id.is_empty() && cue_id == main_id {
            // Same device — one combined stream handles both master (ch main_off/main_off+1)
            // and cue (ch cue_off/cue_off+1) in a single callback.
            let device = find_output_device(&main_id)?;
            let min_ch = (main_off + 2).max(cue_off + 2);
            let config = best_output_config(&device, min_ch, self.device_sample_rate)?;
            log::info!("rebuild_streams: combined config ch={} sr={} fmt={:?}",
                config.channels(), config.sample_rate().0, config.sample_format());
            let stream = build_combined_stream(&device, &config, ch, config.channels() as usize, main_off, cue_off)
                .map_err(|e| e.to_string())?;

            // Pause all old streams, sync positions, then start the new combined stream.
            self._main_stream.lock().unwrap().0.pause().ok();
            {
                let mut guard = self._cue_stream.lock().unwrap();
                if let Some(s) = guard.as_ref() { s.0.pause().ok(); }
                *guard = None;
            }
            self.sync_cue_positions();
            {
                let mut guard = self._main_stream.lock().unwrap();
                *guard = SendStream(stream);
                guard.0.play().map_err(|e| e.to_string())?;
            }
            log::info!("rebuild_streams: combined stream playing");
        } else {
            // Different devices (or no cue configured) — two independent streams.
            // Build all new streams before pausing anything so the gap is minimal.
            let main_device = find_output_device(&main_id)?;
            let main_cfg    = best_output_config(&main_device, main_off + 2, self.device_sample_rate)?;
            log::info!("rebuild_streams: master config ch={} sr={} fmt={:?}",
                main_cfg.channels(), main_cfg.sample_rate().0, main_cfg.sample_format());
            let main_stream = build_stream(&main_device, &main_cfg, ch.clone(), false, main_off)
                .map_err(|e| e.to_string())?;

            let new_cue_stream = if !cue_id.is_empty() {
                let cue_device = find_output_device(&cue_id)?;
                let cue_cfg    = best_output_config(&cue_device, cue_off + 2, self.device_sample_rate)?;
                log::info!("rebuild_streams: cue config ch={} sr={} fmt={:?}",
                    cue_cfg.channels(), cue_cfg.sample_rate().0, cue_cfg.sample_format());
                Some(build_stream(&cue_device, &cue_cfg, ch, true, cue_off)
                    .map_err(|e| e.to_string())?)
            } else {
                None
            };

            // Pause all old streams, sync cue_pos to main_pos, then start new streams.
            self._main_stream.lock().unwrap().0.pause().ok();
            {
                let guard = self._cue_stream.lock().unwrap();
                if let Some(s) = guard.as_ref() { s.0.pause().ok(); }
            }
            self.sync_cue_positions();

            {
                let mut guard = self._main_stream.lock().unwrap();
                *guard = SendStream(main_stream);
                guard.0.play().map_err(|e| e.to_string())?;
            }
            {
                let mut guard = self._cue_stream.lock().unwrap();
                match new_cue_stream {
                    Some(s) => {
                        *guard = Some(SendStream(s));
                        guard.as_ref().unwrap().0.play().map_err(|e| e.to_string())?;
                    }
                    None => *guard = None,
                }
            }
            log::info!("rebuild_streams: separate streams playing");
        }

        Ok(())
    }

    fn sync_cue_positions(&self) {
        for deck_arc in self.decks.values() {
            let mut deck = deck_arc.lock().unwrap();
            deck.cue_pos = deck.main_pos;
        }
    }
}

// Build a paired list of (deck, strip) in a consistent order for use in stream callbacks.
fn channel_pairs(
    decks: &HashMap<String, Arc<Mutex<DeckState>>>,
    strips: &HashMap<String, Arc<Mutex<ChannelStrip>>>,
) -> Vec<(Arc<Mutex<DeckState>>, Arc<Mutex<ChannelStrip>>)> {
    let mut ids: Vec<&String> = decks.keys().collect();
    ids.sort();
    ids.into_iter()
        .filter_map(|id| {
            let deck = decks.get(id)?;
            let strip = strips.get(id)?;
            Some((Arc::clone(deck), Arc::clone(strip)))
        })
        .collect()
}

fn find_output_device(device_id: &str) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    host.devices()
        .map_err(|e| e.to_string())?
        .filter(|d| {
            d.supported_output_configs()
                .map(|mut c| c.next().is_some())
                .unwrap_or(false)
        })
        .find(|d| d.name().map(|n| n == device_id).unwrap_or(false))
        .ok_or_else(|| format!("device not found: {}", device_id))
}

// Find the supported output config with the fewest channels that still satisfies
// min_channels, preferring configs whose sample-rate range includes preferred_sr.
fn best_output_config(
    device: &cpal::Device,
    min_channels: usize,
    preferred_sr: u32,
) -> Result<cpal::SupportedStreamConfig, String> {
    let min_ch = min_channels as u16;
    let target_sr = cpal::SampleRate(preferred_sr);

    let all: Vec<_> = device
        .supported_output_configs()
        .map_err(|e| e.to_string())?
        .collect();

    log::info!(
        "best_output_config: device='{}' min_channels={} preferred_sr={} | supported=[{}]",
        device.name().unwrap_or_default(),
        min_channels,
        preferred_sr,
        all.iter()
            .map(|c| format!("{}ch/{}-{}Hz", c.channels(), c.min_sample_rate().0, c.max_sample_rate().0))
            .collect::<Vec<_>>()
            .join(", ")
    );

    // Prefer configs that include the current sample rate so loaded tracks play
    // at the right pitch. Fall back to any config with enough channels.
    if let Some(range) = all.iter()
        .filter(|c| c.channels() >= min_ch && c.min_sample_rate() <= target_sr && c.max_sample_rate() >= target_sr)
        .min_by_key(|c| c.channels())
    {
        let cfg = range.clone().with_sample_rate(target_sr);
        log::info!("best_output_config: chose {}ch @ {}Hz (sr match)", cfg.channels(), cfg.sample_rate().0);
        return Ok(cfg);
    }

    if let Some(range) = all.iter()
        .filter(|c| c.channels() >= min_ch)
        .min_by_key(|c| c.channels())
    {
        let cfg = range.clone().with_max_sample_rate();
        log::info!("best_output_config: chose {}ch @ {}Hz (no sr match)", cfg.channels(), cfg.sample_rate().0);
        return Ok(cfg);
    }

    // Device has no config with enough channels — fall back to default and let
    // mix_frame clamp gracefully (audio will be silent for out-of-range offsets).
    let cfg = device.default_output_config().map_err(|e| e.to_string())?;
    log::warn!(
        "best_output_config: no config with >={} channels, falling back to default ({}ch)",
        min_channels, cfg.channels()
    );
    Ok(cfg)
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    channels: Vec<(Arc<Mutex<DeckState>>, Arc<Mutex<ChannelStrip>>)>,
    is_cue: bool,
    channel_offset: usize,
) -> Result<cpal::Stream, Box<dyn std::error::Error + Send + Sync>> {
    let stream_config: cpal::StreamConfig = config.clone().into();
    let output_channels = config.channels() as usize;
    let label = if is_cue { "cue" } else { "master" };
    log::info!(
        "build_stream [{}]: output_channels={} channel_offset={} format={:?} sample_rate={}",
        label, output_channels, channel_offset, config.sample_format(), config.sample_rate().0
    );

    match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let stream = device.build_output_stream(
                &stream_config,
                move |data: &mut [f32], _| {
                    fill_output(data, output_channels, &channels, is_cue, channel_offset);
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
                    fill_output(&mut buf, output_channels, &channels, is_cue, channel_offset);
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
    channels: &[(Arc<Mutex<DeckState>>, Arc<Mutex<ChannelStrip>>)],
    is_cue: bool,
    channel_offset: usize,
) {
    data.fill(0.0);
    let frames = data.len() / output_channels.max(1);
    let deck_tick: fn(&mut DeckState) -> (f32, f32) = if is_cue { DeckState::cue_tick } else { DeckState::main_tick };
    let strip_process: fn(&mut ChannelStrip, f32, f32) -> (f32, f32) = if is_cue { ChannelStrip::process_cue } else { ChannelStrip::process_main };

    for (deck_arc, strip_arc) in channels {
        let mut deck = deck_arc.lock().unwrap();
        let mut strip = strip_arc.lock().unwrap();
        for i in 0..frames {
            let (l, r) = deck_tick(&mut deck);
            let (l, r) = strip_process(&mut strip, l, r);
            mix_frame(data, i, output_channels, channel_offset, l, r);
        }
    }

    for sample in data.iter_mut() {
        *sample = sample.clamp(-1.0, 1.0);
    }
}

#[inline]
fn mix_frame(data: &mut [f32], frame: usize, channels: usize, channel_offset: usize, l: f32, r: f32) {
    let base = frame * channels + channel_offset;
    let remaining = channels.saturating_sub(channel_offset);
    if remaining == 0 {
        return;
    }
    if remaining == 1 {
        if base < data.len() {
            data[base] += (l + r) * 0.5;
        }
    } else if base + 1 < data.len() {
        data[base] += l;
        data[base + 1] += r;
    }
}

fn build_combined_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    channels: Vec<(Arc<Mutex<DeckState>>, Arc<Mutex<ChannelStrip>>)>,
    output_channels: usize,
    main_offset: usize,
    cue_offset: usize,
) -> Result<cpal::Stream, Box<dyn std::error::Error + Send + Sync>> {
    let stream_config: cpal::StreamConfig = config.clone().into();
    log::info!(
        "build_combined_stream: output_channels={} main_offset={} cue_offset={} format={:?} sr={}",
        output_channels, main_offset, cue_offset, config.sample_format(), config.sample_rate().0
    );
    match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let stream = device.build_output_stream(
                &stream_config,
                move |data: &mut [f32], _| {
                    fill_output_combined(data, output_channels, &channels, main_offset, cue_offset);
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
                    fill_output_combined(&mut buf, output_channels, &channels, main_offset, cue_offset);
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

fn fill_output_combined(
    data: &mut [f32],
    output_channels: usize,
    channels: &[(Arc<Mutex<DeckState>>, Arc<Mutex<ChannelStrip>>)],
    main_offset: usize,
    cue_offset: usize,
) {
    data.fill(0.0);
    let frames = data.len() / output_channels.max(1);
    for (deck_arc, strip_arc) in channels {
        let mut deck = deck_arc.lock().unwrap();
        let mut strip = strip_arc.lock().unwrap();
        for i in 0..frames {
            let (l, r) = deck.main_tick();
            let (ml, mr) = strip.process_main(l, r);
            mix_frame(data, i, output_channels, main_offset, ml, mr);
            let (l, r) = deck.cue_tick();
            let (cl, cr) = strip.process_cue(l, r);
            mix_frame(data, i, output_channels, cue_offset, cl, cr);
        }
    }
    for s in data.iter_mut() {
        *s = s.clamp(-1.0, 1.0);
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

// Compute `num_points` peak amplitude values for the region [start_sec, end_sec]
// of the (resampled, interleaved) sample buffer at `device_sample_rate`.
// Always returns exactly `num_points` values. Resolution adapts to the zoom
// level: each point covers (end_sec - start_sec) / num_points seconds of audio,
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
