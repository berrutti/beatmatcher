use super::dsp::{EqState, FilterState};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

const GAIN_SMOOTHING_TAU_SEC: f32 = 0.010;

pub struct ChannelStrip {
    target_gain: f32,
    current_gain: f32,
    gain_smooth_coeff: f32,
    muted: bool,
    mute_gain: f32,
    pub(crate) cue_active: bool,
    eq: EqState,
    eq_cue: EqState,
    filter: FilterState,
    filter_cue: FilterState,
    level_l: Arc<AtomicU32>,
    level_r: Arc<AtomicU32>,
}

impl ChannelStrip {
    pub fn new(sample_rate: f32) -> Self {
        let gain_smooth_coeff = 1.0 - (-1.0 / (sample_rate * GAIN_SMOOTHING_TAU_SEC)).exp();
        Self {
            target_gain: 1.0,
            current_gain: 1.0,
            gain_smooth_coeff,
            muted: false,
            mute_gain: 1.0,
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
            "low" => {
                self.eq.set_low(db);
                self.eq_cue.set_low(db);
            }
            "mid" => {
                self.eq.set_mid(db);
                self.eq_cue.set_mid(db);
            }
            "high" => {
                self.eq.set_high(db);
                self.eq_cue.set_high(db);
            }
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

    // Session-view mute (per-deck mute/solo). Independent of the fader gain so
    // replayed set_volume events cannot override it, and deliberately NOT
    // cleared by reset(): scrubbing resets strips to reconstruct session
    // state, and the mute must survive that.
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    pub(crate) fn reset(&mut self) {
        self.set_gain(1.0);
        self.set_eq_band("low", 0.0);
        self.set_eq_band("mid", 0.0);
        self.set_eq_band("high", 0.0);
        self.set_filter(0.0);
        self.set_filter_active(false);
    }

    // Applied to the master output path: EQ, filter, then fader gain.
    #[inline]
    pub fn process_main(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (el, er) = self.eq.process(l, r);
        let (fl, fr) = self.filter.process(el, er);
        self.current_gain += (self.target_gain - self.current_gain) * self.gain_smooth_coeff;
        // Mute fades over the same time constant as the fader to avoid clicks.
        let mute_target = if self.muted { 0.0 } else { 1.0 };
        self.mute_gain += (mute_target - self.mute_gain) * self.gain_smooth_coeff;
        let gain = self.current_gain * self.mute_gain;
        (fl * gain, fr * gain)
    }

    // Applied to the cue output path: EQ then filter (pre-fader), gated by
    // cue_active. Always called so filter state stays in sync; output is
    // silenced when cue_active is false.
    #[inline]
    pub fn process_cue(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (l, r) = self.eq_cue.process(l, r);
        let (l, r) = self.filter_cue.process(l, r);
        if self.cue_active {
            (l, r)
        } else {
            (0.0, 0.0)
        }
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

// Outcome returned by press_cue so the Tauri command layer can relay the
// relevant position data back to the frontend for display sync.
#[derive(Debug, PartialEq)]
pub enum CuePressOutcome {
    PreviewStarted,
    CueMoved { new_cue_point_sec: f64 },
    StoppedAtCue { cue_point_sec: f64 },
    NoTrack,
}

pub struct DeckState {
    pub(crate) samples: Arc<Vec<f32>>, // interleaved f32 at device_sample_rate
    pub(crate) channels: usize,
    pub(crate) device_sample_rate: u32,
    pub(crate) total_frames: usize,
    pub(crate) duration: f64,
    pub(crate) loaded_path: Option<String>,

    pub(crate) is_playing: bool,
    pub(crate) is_cueing: bool,
    pub(crate) main_pos: f64,  // fractional frame index
    pub(crate) cue_pos: f64,   // fractional frame index (independent of main_pos)
    pub(crate) cue_point: f64, // in frames; the stored cue point and loop-in position
    pub(crate) loop_active: bool,
    pub(crate) loop_end: f64, // in frames; loop_start is always cue_point
    pub(crate) bpm: Option<f64>,
    pub(crate) beat_offset_frames: f64,
    pub(crate) playback_rate: f64,
    pub(crate) nudge_factor: f64, // 1 + nudge_percent/100
    pub(crate) quantize: bool,

    // Spectral band buffers (mono, at device_sample_rate) and per-band normalization scales.
    pub(crate) bass_band: Arc<Vec<f32>>,
    pub(crate) mid_band: Arc<Vec<f32>>,
    pub(crate) high_band: Arc<Vec<f32>>,
    pub(crate) bass_scale: f32,
    pub(crate) mid_scale: f32,
    pub(crate) high_scale: f32,

    // Set to true by the audio thread when the track reaches its natural end.
    // The monitoring task in lib.rs polls this and emits a "track-ended" event.
    pub(crate) just_ended: Arc<AtomicBool>,
}

impl DeckState {
    pub fn empty(device_sample_rate: u32) -> Self {
        Self {
            samples: Arc::new(Vec::new()),
            channels: 2,
            device_sample_rate,
            total_frames: 0,
            duration: 0.0,
            loaded_path: None,
            is_playing: false,
            is_cueing: false,
            main_pos: 0.0,
            cue_pos: 0.0,
            cue_point: 0.0,
            loop_active: false,
            loop_end: 0.0,
            bpm: None,
            beat_offset_frames: 0.0,
            playback_rate: 1.0,
            nudge_factor: 1.0,
            quantize: true,
            bass_band: Arc::new(Vec::new()),
            mid_band: Arc::new(Vec::new()),
            high_band: Arc::new(Vec::new()),
            bass_scale: 1.0,
            mid_scale: 1.0,
            high_scale: 1.0,
            just_ended: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn eject(&mut self) {
        self.is_playing = false;
        self.is_cueing = false;
        self.samples = Arc::new(Vec::new());
        self.total_frames = 0;
        self.duration = 0.0;
        self.main_pos = 0.0;
        self.cue_pos = 0.0;
        self.cue_point = 0.0;
        self.loop_active = false;
        self.loop_end = 0.0;
        self.bpm = None;
        self.beat_offset_frames = 0.0;
        self.loaded_path = None;
    }

    pub(crate) fn reset(&mut self) {
        self.eject();
        self.playback_rate = 1.0;
        self.nudge_factor = 1.0;
    }

    // Threshold for "position is at the cue point" used by press_cue.
    // 50 frames at 44100 Hz ≈ 1.1 ms; matches the frontend's 0.001 s tolerance.
    const CUE_THRESHOLD_FRAMES: f64 = 50.0;

    pub fn press_cue(&mut self) -> CuePressOutcome {
        if self.total_frames == 0 {
            return CuePressOutcome::NoTrack;
        }
        if self.is_cueing {
            return CuePressOutcome::PreviewStarted;
        }
        if self.is_playing {
            self.is_playing = false;
            self.main_pos = self.cue_point;
            self.cue_pos = self.cue_point;
            return CuePressOutcome::StoppedAtCue {
                cue_point_sec: self.cue_point / self.device_sample_rate as f64,
            };
        }
        if (self.main_pos - self.cue_point).abs() <= Self::CUE_THRESHOLD_FRAMES {
            self.is_playing = true;
            self.is_cueing = true;
            self.main_pos = self.cue_point;
            self.cue_pos = self.cue_point;
            return CuePressOutcome::PreviewStarted;
        }
        self.cue_point = self.main_pos;
        CuePressOutcome::CueMoved {
            new_cue_point_sec: self.cue_point / self.device_sample_rate as f64,
        }
    }

    pub fn release_cue(&mut self) {
        if !self.is_cueing {
            return;
        }
        self.is_playing = false;
        self.is_cueing = false;
        self.main_pos = self.cue_point;
        self.cue_pos = self.cue_point;
    }

    pub fn toggle_play(&mut self) {
        if self.total_frames == 0 {
            return;
        }
        if self.is_cueing {
            // latch-on: release the cue and continue playing from current position
            self.is_cueing = false;
        } else {
            self.is_playing = !self.is_playing;
        }
    }

    pub fn set_cue_and_stop(&mut self) {
        if !self.is_playing {
            return;
        }
        self.cue_point = self.main_pos;
        self.is_playing = false;
        self.is_cueing = false;
        self.cue_pos = self.main_pos;
    }

    pub fn stop_at_cue(&mut self) {
        if !self.is_playing {
            return;
        }
        self.is_playing = false;
        self.is_cueing = false;
        self.main_pos = self.cue_point;
        self.cue_pos = self.cue_point;
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
            let l = self.samples[lo_idx]
                + interp_factor * (self.samples[hi_idx] - self.samples[lo_idx]);
            let r = self.samples[lo_idx + 1]
                + interp_factor * (self.samples[hi_idx + 1] - self.samples[lo_idx + 1]);
            (l, r)
        }
    }

    fn next_pos(&mut self, pos: f64, is_main: bool) -> f64 {
        let step = self.playback_rate * self.nudge_factor;
        let new_pos = pos + step;

        if self.loop_active && new_pos >= self.loop_end {
            let dur = self.loop_end - self.cue_point;
            return if dur > 0.0 {
                self.cue_point + (new_pos - self.loop_end) % dur
            } else {
                self.cue_point
            };
        }

        if new_pos >= self.total_frames as f64 {
            if is_main {
                self.is_playing = false;
                self.just_ended.store(true, Ordering::Release);
                self.main_pos = self.cue_point;
                self.cue_pos = self.cue_point;
            }
            return self.cue_point;
        }

        new_pos
    }

    // Advance a playing deck by `overshoot_frames` of master-output time (scaled
    // by the effective rate), wrapping inside an active loop and clamping to the
    // track end. Session playback uses this to keep a deck that was started or
    // repositioned a fraction of a buffer late aligned with decks already playing.
    pub(crate) fn compensate_late_start(&mut self, overshoot_frames: f64) {
        if !self.is_playing || overshoot_frames <= 0.0 {
            return;
        }
        let mut pos = self.main_pos + overshoot_frames * self.playback_rate * self.nudge_factor;
        if self.loop_active && self.loop_end > self.cue_point && pos >= self.loop_end {
            let dur = self.loop_end - self.cue_point;
            pos = self.cue_point + (pos - self.loop_end).rem_euclid(dur);
        }
        if self.total_frames > 0 {
            pos = pos.min(self.total_frames as f64 - 1.0);
        }
        self.main_pos = pos;
        self.cue_pos = pos;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 44100;

    impl DeckState {
        // Creates a DeckState loaded with a 440 Hz sine wave. No audio hardware.
        pub fn loaded_for_testing(device_sample_rate: u32, duration_secs: f64) -> Self {
            let total_frames = (device_sample_rate as f64 * duration_secs) as usize;
            let freq = 440.0f64;
            let samples: Vec<f32> = (0..total_frames)
                .flat_map(|i| {
                    let t = i as f64 / device_sample_rate as f64;
                    let s = (2.0 * std::f64::consts::PI * freq * t).sin() as f32;
                    [s, s]
                })
                .collect();
            let mut d = DeckState::empty(device_sample_rate);
            d.samples = Arc::new(samples);
            d.total_frames = total_frames;
            d.duration = duration_secs;
            d
        }
    }

    // --- DeckState tick ---

    #[test]
    fn deck_stops_at_natural_end_of_track() {
        let mut d = DeckState::loaded_for_testing(SR, 1.0);
        d.is_playing = true;
        d.main_pos = (d.total_frames - 1) as f64;
        d.main_tick();
        assert!(
            !d.is_playing,
            "deck should stop when it reaches the last frame"
        );
    }

    #[test]
    fn deck_is_silent_when_not_playing() {
        let mut d = DeckState::loaded_for_testing(SR, 5.0);
        d.is_playing = false;
        let (l, r) = d.main_tick();
        assert_eq!(l, 0.0);
        assert_eq!(r, 0.0);
    }

    #[test]
    fn deck_position_advances_while_playing() {
        let mut d = DeckState::loaded_for_testing(SR, 5.0);
        d.is_playing = true;
        d.main_pos = 0.0;
        for _ in 0..1000 {
            d.main_tick();
        }
        assert!(
            d.main_pos > 900.0,
            "expected position to advance, got {}",
            d.main_pos
        );
    }

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
        assert!(
            l < 0.001,
            "expected gain near 0.0 after convergence, got {}",
            l
        );
    }

    #[test]
    fn channel_strip_gain_starts_at_full_volume() {
        let mut strip = ChannelStrip::new(48000.0);
        let (l, r) = strip.process_main(1.0, 1.0);
        assert!(l > 0.99, "expected l near 1.0, got {}", l);
        assert!(r > 0.99, "expected r near 1.0, got {}", r);
    }

    #[test]
    fn channel_strip_mute_fades_to_silence() {
        let mut strip = ChannelStrip::new(48000.0);
        strip.set_muted(true);
        let (first, _) = strip.process_main(1.0, 1.0);
        assert!(first > 0.5, "mute must fade, not jump: got {}", first);
        for _ in 0..24_000 {
            strip.process_main(1.0, 1.0);
        }
        let (settled, _) = strip.process_main(1.0, 1.0);
        assert!(
            settled < 0.001,
            "expected silence when muted, got {}",
            settled
        );
    }

    #[test]
    fn channel_strip_mute_survives_reset_and_gain_changes() {
        let mut strip = ChannelStrip::new(48000.0);
        strip.set_muted(true);
        strip.reset();
        strip.set_gain(1.0);
        for _ in 0..24_000 {
            strip.process_main(1.0, 1.0);
        }
        let (settled, _) = strip.process_main(1.0, 1.0);
        assert!(settled < 0.001, "mute must survive reset, got {}", settled);

        strip.set_muted(false);
        for _ in 0..24_000 {
            strip.process_main(1.0, 1.0);
        }
        let (restored, _) = strip.process_main(1.0, 1.0);
        assert!(
            restored > 0.99,
            "unmute must restore audio, got {}",
            restored
        );
    }
}

// State machine tests for the cue/play commands being ported from TypeScript
// Every test describes one state machine transition. The state is encoded in
// three fields: total_frames (0 = empty), is_playing, is_cueing.
#[cfg(test)]
mod cue_state_machine {
    use super::*;

    const SR: u32 = 44100;
    const BPM: f64 = 120.0;

    fn beat_frames() -> f64 {
        (60.0 / BPM) * SR as f64
    }

    fn stopped(duration_secs: f64) -> DeckState {
        let mut d = DeckState::loaded_for_testing(SR, duration_secs);
        d.is_playing = false;
        d.is_cueing = false;
        d.cue_point = 0.0;
        d
    }

    fn playing(duration_secs: f64) -> DeckState {
        let mut d = DeckState::loaded_for_testing(SR, duration_secs);
        d.is_playing = true;
        d.is_cueing = false;
        d.cue_point = 0.0;
        d
    }

    fn cueing(duration_secs: f64) -> DeckState {
        let mut d = DeckState::loaded_for_testing(SR, duration_secs);
        d.is_playing = true;
        d.is_cueing = true;
        d.cue_point = 0.0;
        d
    }

    // ── toggle_play ──────────────────────────────────────────────────────────

    #[test]
    fn toggle_play_on_empty_deck_does_nothing() {
        let mut d = DeckState::empty(SR);
        d.toggle_play();
        assert!(!d.is_playing);
    }

    #[test]
    fn toggle_play_stopped_to_playing() {
        let mut d = stopped(10.0);
        d.toggle_play();
        assert!(d.is_playing);
        assert!(!d.is_cueing);
    }

    #[test]
    fn toggle_play_playing_to_stopped() {
        let mut d = playing(10.0);
        d.toggle_play();
        assert!(!d.is_playing);
        assert!(!d.is_cueing);
    }

    // Pressing PLAY during cue preview "latches on". Playback continues from
    // the current position instead of returning to the cue point.
    #[test]
    fn toggle_play_during_cue_preview_latches_to_playing() {
        let mut d = cueing(10.0);
        d.main_pos = beat_frames() * 2.0 + 1000.0; // slightly past cue
        d.toggle_play();
        assert!(d.is_playing);
        assert!(!d.is_cueing, "cueing flag must be cleared");
        // position stays wherever playback was, not snapped back to cue
        assert!(d.main_pos > beat_frames() * 2.0);
    }

    // ── press_cue ────────────────────────────────────────────────────────────

    #[test]
    fn press_cue_on_empty_deck_does_nothing() {
        let mut d = DeckState::empty(SR);
        let outcome = d.press_cue();
        assert_eq!(outcome, CuePressOutcome::NoTrack);
        assert!(!d.is_playing);
    }

    #[test]
    fn press_cue_stopped_at_cue_starts_preview() {
        let mut d = stopped(10.0);
        d.cue_point = beat_frames() * 2.0;
        d.main_pos = d.cue_point; // exactly at cue
        let outcome = d.press_cue();
        assert_eq!(outcome, CuePressOutcome::PreviewStarted);
        assert!(d.is_playing);
        assert!(d.is_cueing);
        // playback must start FROM the cue point
        assert!((d.main_pos - d.cue_point).abs() < 1.0);
    }

    #[test]
    fn press_cue_stopped_within_threshold_of_cue_starts_preview() {
        let mut d = stopped(10.0);
        d.cue_point = beat_frames() * 2.0;
        d.main_pos = d.cue_point + DeckState::CUE_THRESHOLD_FRAMES * 0.5; // inside tolerance
        let outcome = d.press_cue();
        assert_eq!(outcome, CuePressOutcome::PreviewStarted);
    }

    #[test]
    fn press_cue_stopped_away_from_cue_moves_cue_no_playback() {
        let mut d = stopped(10.0);
        let original_cue = beat_frames() * 2.0;
        d.cue_point = original_cue;
        d.main_pos = beat_frames() * 5.0;
        let outcome = d.press_cue();
        let new_cue_sec = beat_frames() * 5.0 / SR as f64;
        assert_eq!(
            outcome,
            CuePressOutcome::CueMoved {
                new_cue_point_sec: new_cue_sec
            }
        );
        assert!(!d.is_playing);
        assert_eq!(
            d.cue_point,
            beat_frames() * 5.0,
            "cue_point must update to main_pos"
        );
    }

    #[test]
    fn press_cue_while_playing_stops_and_returns_to_cue() {
        let mut d = playing(10.0);
        d.cue_point = beat_frames() * 2.0;
        d.main_pos = beat_frames() * 7.0;
        let outcome = d.press_cue();
        let cue_sec = d.cue_point / SR as f64;
        assert_eq!(
            outcome,
            CuePressOutcome::StoppedAtCue {
                cue_point_sec: cue_sec
            }
        );
        assert!(!d.is_playing);
        assert!(!d.is_cueing);
        assert!((d.main_pos - d.cue_point).abs() < 1.0);
    }

    // Pressing CUE again during preview is a no-op. You can't start another
    // preview while one is already running.
    #[test]
    fn press_cue_during_preview_is_noop() {
        let mut d = cueing(10.0);
        d.cue_point = beat_frames() * 2.0;
        d.main_pos = d.cue_point + 200.0;
        let pos_before = d.main_pos;
        let outcome = d.press_cue();
        assert_eq!(outcome, CuePressOutcome::PreviewStarted);
        assert!(d.is_playing);
        assert!(d.is_cueing);
        // position must not jump
        assert!((d.main_pos - pos_before).abs() < 1.0);
    }

    // ── release_cue ──────────────────────────────────────────────────────────

    #[test]
    fn release_cue_during_preview_stops_and_returns_to_cue() {
        let mut d = cueing(10.0);
        d.cue_point = beat_frames() * 2.0;
        d.main_pos = d.cue_point + 800.0; // played a bit
        d.release_cue();
        assert!(!d.is_playing);
        assert!(!d.is_cueing);
        assert!((d.main_pos - d.cue_point).abs() < 1.0);
    }

    // Idempotent: releasing when already stopped must not crash or change state.
    #[test]
    fn release_cue_when_stopped_is_noop() {
        let mut d = stopped(10.0);
        d.cue_point = beat_frames() * 2.0;
        d.main_pos = beat_frames() * 3.0;
        let pos_before = d.main_pos;
        d.release_cue(); // first call
        d.release_cue(); // second call. Must also be fine
        assert!(!d.is_playing);
        assert_eq!(
            d.main_pos, pos_before,
            "position must not move on no-op release"
        );
    }

    #[test]
    fn release_cue_while_playing_normally_is_noop() {
        let mut d = playing(10.0);
        d.cue_point = beat_frames() * 2.0;
        d.main_pos = beat_frames() * 5.0;
        d.release_cue();
        assert!(d.is_playing, "normal playback must continue");
        assert!(!d.is_cueing);
    }

    #[test]
    fn release_cue_on_empty_deck_is_noop() {
        let mut d = DeckState::empty(SR);
        d.release_cue(); // must not panic
        assert!(!d.is_playing);
    }

    // ── set_cue_and_stop ─────────────────────────────────────────────────────

    #[test]
    fn set_cue_and_stop_while_playing_freezes_cue_at_playhead() {
        let mut d = playing(10.0);
        d.main_pos = beat_frames() * 3.5;
        d.set_cue_and_stop();
        assert!(!d.is_playing);
        assert!(!d.is_cueing);
        assert!(
            (d.cue_point - beat_frames() * 3.5).abs() < 1.0,
            "cue_point must be set to playhead"
        );
        assert!(
            (d.main_pos - d.cue_point).abs() < 1.0,
            "position must stay at new cue point"
        );
    }

    // No-op when already stopped. Calling it twice must be safe.
    #[test]
    fn set_cue_and_stop_when_stopped_is_noop() {
        let mut d = stopped(10.0);
        d.cue_point = beat_frames() * 2.0;
        d.main_pos = beat_frames() * 2.0;
        d.set_cue_and_stop();
        assert!(!d.is_playing);
        assert_eq!(
            d.cue_point,
            beat_frames() * 2.0,
            "cue_point must not change when stopped"
        );
    }

    // During cueing, set_cue_and_stop ends the preview and locks cue at
    // the current (slightly advanced) position.
    #[test]
    fn set_cue_and_stop_during_preview_ends_preview_and_updates_cue() {
        let mut d = cueing(10.0);
        d.cue_point = beat_frames() * 2.0;
        d.main_pos = d.cue_point + 300.0;
        let expected_cue = d.main_pos;
        d.set_cue_and_stop();
        assert!(!d.is_playing);
        assert!(!d.is_cueing);
        assert!(
            (d.cue_point - expected_cue).abs() < 1.0,
            "cue_point must be set to playhead at time of call"
        );
    }

    // ── stop_at_cue ──────────────────────────────────────────────────────────

    #[test]
    fn stop_at_cue_while_playing_stops_and_returns_to_cue() {
        let mut d = playing(10.0);
        d.cue_point = beat_frames() * 2.0;
        d.main_pos = beat_frames() * 7.0;
        let cue_before = d.cue_point;
        d.stop_at_cue();
        assert!(!d.is_playing);
        assert!(!d.is_cueing);
        assert_eq!(d.cue_point, cue_before, "cue_point must not change");
        assert!((d.main_pos - cue_before).abs() < 1.0);
    }

    #[test]
    fn stop_at_cue_when_already_stopped_is_noop() {
        let mut d = stopped(10.0);
        d.cue_point = beat_frames() * 2.0;
        d.main_pos = beat_frames() * 4.0; // position away from cue
        d.stop_at_cue(); // first call
        d.stop_at_cue(); // second call. Must not change anything further
        assert!(!d.is_playing);
    }

    #[test]
    fn stop_at_cue_during_preview_stops_and_returns_to_cue() {
        let mut d = cueing(10.0);
        d.cue_point = beat_frames() * 2.0;
        d.main_pos = d.cue_point + 400.0;
        let cue_before = d.cue_point;
        d.stop_at_cue();
        assert!(!d.is_playing);
        assert!(!d.is_cueing);
        assert_eq!(d.cue_point, cue_before);
        assert!((d.main_pos - cue_before).abs() < 1.0);
    }

    #[test]
    fn stop_at_cue_on_empty_deck_is_noop() {
        let mut d = DeckState::empty(SR);
        d.stop_at_cue(); // must not panic
        assert!(!d.is_playing);
    }

    // ── cue point persistence across state changes ────────────────────────────

    // Cue point set during preview (press_cue moves it) must survive a
    // press → release cycle and be the correct return target.
    #[test]
    fn cue_point_moved_then_preview_returns_to_new_cue() {
        let mut d = stopped(10.0);
        d.cue_point = 0.0;

        // First press: away from cue → moves cue to current position
        d.main_pos = beat_frames() * 3.0;
        d.press_cue();
        assert_eq!(d.cue_point, beat_frames() * 3.0);

        // Second press: now at the new cue → starts preview
        d.press_cue();
        assert!(d.is_cueing);

        // Release: must return to the new cue point, not 0
        let pos_during_preview = d.main_pos + 500.0;
        d.main_pos = pos_during_preview;
        d.release_cue();
        assert!((d.main_pos - beat_frames() * 3.0).abs() < 1.0);
    }
}

// ── Loop behaviour ────────────────────────────────────────────────────────────

#[cfg(test)]
mod loop_behavior {
    use super::*;

    const SR: u32 = 44100;
    const BPM: f64 = 120.0;

    fn beat_frames() -> f64 {
        (60.0 / BPM) * SR as f64
    }

    fn deck_with_grid(duration_secs: f64) -> DeckState {
        let mut d = DeckState::loaded_for_testing(SR, duration_secs);
        d.bpm = Some(BPM);
        d.beat_offset_frames = 0.0;
        d
    }

    // Loop start IS the cue point: next_pos must wrap to cue_point, not a
    // separate loop_start variable.
    #[test]
    fn loop_wraps_to_cue_point_not_separate_variable() {
        let mut d = deck_with_grid(10.0);
        d.cue_point = beat_frames();
        d.loop_end = beat_frames() * 2.0;
        d.loop_active = true;
        d.is_playing = true;
        d.main_pos = d.loop_end - 1.0; // one frame before loop end
        d.main_tick();
        assert!(
            d.main_pos >= d.cue_point && d.main_pos < d.loop_end,
            "expected position inside loop [{}, {}), got {}",
            d.cue_point,
            d.loop_end,
            d.main_pos,
        );
    }

    #[test]
    fn loop_does_not_wrap_when_inactive() {
        let mut d = deck_with_grid(10.0);
        d.cue_point = beat_frames();
        d.loop_end = beat_frames() * 2.0;
        d.loop_active = false;
        d.is_playing = true;
        d.main_pos = d.loop_end - 1.0;
        d.main_tick();
        // should advance past loop_end without wrapping
        assert!(d.main_pos >= d.loop_end);
    }
}
