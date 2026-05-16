use std::sync::{Arc, atomic::{AtomicBool, AtomicU32, Ordering}};
use super::dsp::{EqState, FilterState};

const GAIN_SMOOTHING_TAU_SEC: f32 = 0.010;

pub struct ChannelStrip {
    target_gain: f32,
    current_gain: f32,
    gain_smooth_coeff: f32,
    pub cue_active: bool,
    eq: EqState,
    eq_cue: EqState,
    filter: FilterState,
    filter_cue: FilterState,
    pub level_l: Arc<AtomicU32>,
    pub level_r: Arc<AtomicU32>,
}

impl ChannelStrip {
    pub fn new(sample_rate: f32) -> Self {
        let gain_smooth_coeff = 1.0 - (-1.0 / (sample_rate * GAIN_SMOOTHING_TAU_SEC)).exp();
        Self {
            target_gain: 1.0,
            current_gain: 1.0,
            gain_smooth_coeff,
            cue_active: false,
            eq: EqState::new(sample_rate),
            eq_cue: EqState::new(sample_rate),
            filter: FilterState::new(sample_rate),
            filter_cue: FilterState::new(sample_rate),
            level_l: Arc::new(AtomicU32::new(0)),
            level_r: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn store_level(&self, l: f32, r: f32) {
        self.level_l.store(l.to_bits(), Ordering::Relaxed);
        self.level_r.store(r.to_bits(), Ordering::Relaxed);
    }

    pub fn get_level(&self) -> [f32; 2] {
        [
            f32::from_bits(self.level_l.load(Ordering::Relaxed)),
            f32::from_bits(self.level_r.load(Ordering::Relaxed)),
        ]
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
        self.filter.set_knob(v);
        self.filter_cue.set_knob(v);
    }

    pub fn set_filter_active(&mut self, active: bool) {
        self.filter.set_active(active);
        self.filter_cue.set_active(active);
    }

    pub fn set_gain(&mut self, v: f32) {
        self.target_gain = v.clamp(0.0, 1.0);
    }

    // Applied to the master output path: EQ, filter, then fader gain.
    #[inline]
    pub fn process_main(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (el, er) = self.eq.process(l, r);
        let (fl, fr) = self.filter.process(el, er);
        self.current_gain += (self.target_gain - self.current_gain) * self.gain_smooth_coeff;
        (fl * self.current_gain, fr * self.current_gain)
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
    pub bpm: Option<f64>,
    pub beat_offset_frames: f64,
    pub playback_rate: f64,
    pub nudge_factor: f64, // 1 + nudge_percent/100

    // Spectral band buffers (mono, at device_sample_rate) and per-band normalization scales.
    pub bass_band: Arc<Vec<f32>>,
    pub mid_band: Arc<Vec<f32>>,
    pub high_band: Arc<Vec<f32>>,
    pub bass_scale: f32,
    pub mid_scale: f32,
    pub high_scale: f32,

    // Set to true by the audio thread when the track reaches its natural end.
    // The monitoring task in lib.rs polls this and emits a "track-ended" event.
    pub just_ended: Arc<AtomicBool>,
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
            bpm: None,
            beat_offset_frames: 0.0,
            playback_rate: 1.0,
            nudge_factor: 1.0,
            bass_band: Arc::new(Vec::new()),
            mid_band: Arc::new(Vec::new()),
            high_band: Arc::new(Vec::new()),
            bass_scale: 1.0,
            mid_scale: 1.0,
            high_scale: 1.0,
            just_ended: Arc::new(AtomicBool::new(false)),
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
                self.just_ended.store(true, Ordering::Release);
            }
            return self.total_frames as f64;
        }

        new_pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ChannelStrip gain smoothing ---

    #[test]
    fn channel_strip_gain_does_not_jump_on_change() {
        let mut strip = ChannelStrip::new(48000.0);
        strip.set_gain(0.0);
        let (l, _) = strip.process_main(1.0, 1.0);
        assert!(l > 0.5, "expected gain near 1.0 on first sample, got {}", l);
    }

    #[test]
    fn channel_strip_gain_converges_to_target() {
        let mut strip = ChannelStrip::new(48000.0);
        strip.set_gain(0.0);
        for _ in 0..24_000 {
            strip.process_main(1.0, 1.0);
        }
        let (l, _) = strip.process_main(1.0, 1.0);
        assert!(l < 0.001, "expected gain near 0.0 after convergence, got {}", l);
    }

    #[test]
    fn channel_strip_gain_starts_at_full_volume() {
        let mut strip = ChannelStrip::new(48000.0);
        let (l, r) = strip.process_main(1.0, 1.0);
        assert!(l > 0.99, "expected l near 1.0, got {}", l);
        assert!(r > 0.99, "expected r near 1.0, got {}", r);
    }
}
