use super::unit::{make_unit, AudioUnit};
use session_core::{MixerManifest, ParamScope};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

// Gain smoothing, filter smoothing and IIR decay all happen inside the
// per-sample process calls, so a stopped strip cannot be skipped until they
// have settled. One second is ~20 tau of the slowest (the filter's 50 ms bypass
// crossfade) plus room for the IIR to reach the f32 noise floor.
const SETTLE_SECONDS: f32 = 1.0;

// Interleaved stereo, `frames * 2` long, accumulated into rather than
// overwritten, since several decks mix into the same buffer.
pub struct RenderTargets<'a> {
    pub main: Option<&'a mut [f32]>,
    pub cue: Option<&'a mut [f32]>,
}

struct Slot {
    name: &'static str,
    main: Box<dyn AudioUnit>,
    // A second instance of the same unit for the pre-fader cue path, so cue
    // filter state tracks the main path instead of sharing its delay lines.
    cue: Option<Box<dyn AudioUnit>>,
}

pub struct ChannelStrip {
    manifest: &'static MixerManifest,
    slots: Vec<Slot>,
    fader_slot: usize,
    pub(crate) cue_active: bool,
    level_l: Arc<AtomicU32>,
    level_r: Arc<AtomicU32>,
    settle_frames: usize,
    settle_window_frames: usize,
}

impl ChannelStrip {
    pub fn new(sample_rate: f32) -> Self {
        Self::from_manifest(&session_core::CLASSIC_3BAND, sample_rate)
    }

    /// Panics on a manifest this build cannot realize. Callers resolve the
    /// manifest first (`session_core::resolve_manifest`), which is where an
    /// unusable one is reported to the user.
    pub fn from_manifest(manifest: &'static MixerManifest, sample_rate: f32) -> Self {
        let cue_tap = manifest
            .strip
            .iter()
            .position(|slot| slot.slot == manifest.cue_tap)
            .unwrap_or_else(|| panic!("manifest '{}' has no cue tap slot", manifest.id));
        let fader_slot = manifest
            .strip
            .iter()
            .position(|slot| slot.slot == session_core::FADER_GAIN.0)
            .unwrap_or_else(|| panic!("manifest '{}' has no fader slot", manifest.id));

        let slots = manifest
            .strip
            .iter()
            .enumerate()
            .map(|(index, descriptor)| {
                let build = || {
                    make_unit(descriptor.unit_id, sample_rate).unwrap_or_else(|| {
                        panic!(
                            "no unit '{}' for slot '{}'",
                            descriptor.unit_id, descriptor.slot
                        )
                    })
                };
                Slot {
                    name: descriptor.slot,
                    main: build(),
                    cue: (index < cue_tap).then(build),
                }
            })
            .collect();

        // Units construct themselves at their descriptor defaults, so a strip
        // is usable without a reset pass. `constructed_units_match_the_manifest_defaults`
        // is what holds those two in step.
        let settle_window_frames = (sample_rate * SETTLE_SECONDS) as usize;
        Self {
            manifest,
            slots,
            fader_slot,
            cue_active: false,
            level_l: Arc::new(AtomicU32::new(0)),
            level_r: Arc::new(AtomicU32::new(0)),
            settle_frames: settle_window_frames,
            settle_window_frames,
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

    pub(crate) fn target_gain(&self) -> f32 {
        self.param(session_core::FADER_GAIN.0, session_core::FADER_GAIN.1)
            .unwrap_or(1.0)
    }

    pub(crate) fn param(&self, slot: &str, param: &str) -> Option<f32> {
        self.slots
            .iter()
            .find(|entry| entry.name == slot)?
            .main
            .param(param)
    }

    fn restart_settle(&mut self) {
        self.settle_frames = self.settle_window_frames;
    }

    fn settled(&self) -> bool {
        self.settle_frames == 0
    }

    fn consume_settle(&mut self, frames: usize) {
        self.settle_frames = self.settle_frames.saturating_sub(frames);
    }

    /// Both output paths, so the cue signal tracks the main one. An address the
    /// manifest does not describe is ignored, which is what lets a session
    /// recorded on a richer mixer replay everything else.
    pub fn set_param(&mut self, slot: &str, param: &str, value: f32) {
        if self
            .manifest
            .descriptor(ParamScope::Deck, slot, param)
            .is_none()
        {
            return;
        }
        let Some(entry) = self.slots.iter_mut().find(|entry| entry.name == slot) else {
            return;
        };
        entry.main.set_param(param, value);
        if let Some(cue) = entry.cue.as_mut() {
            cue.set_param(param, value);
        }
        self.restart_settle();
    }

    pub fn set_eq_band(&mut self, band: &str, db: f32) {
        self.set_param("eq", band, db);
    }

    pub fn set_filter(&mut self, v: f32) {
        self.set_param("filter", "value", v);
    }

    pub fn set_filter_active(&mut self, active: bool) {
        self.set_param("filter", "active", if active { 1.0 } else { 0.0 });
    }

    pub fn set_gain(&mut self, v: f32) {
        self.set_param(session_core::FADER_GAIN.0, session_core::FADER_GAIN.1, v);
    }

    // Session-view mute (per-deck mute/solo). Independent of the fader gain so
    // replayed fader events cannot override it, and deliberately NOT
    // cleared by reset(): scrubbing resets strips to reconstruct session
    // state, and the mute must survive that.
    pub fn set_muted(&mut self, muted: bool) {
        self.restart_settle();
        self.slots[self.fader_slot].main.set_muted(muted);
    }

    pub(crate) fn reset(&mut self) {
        for slot in self.manifest.strip {
            for param in slot.params {
                self.set_param(slot.slot, param.id, param.default as f32);
            }
        }
    }

    // The full strip, in manifest order.
    #[inline]
    pub fn process_main(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (mut l, mut r) = (l, r);
        for slot in &mut self.slots {
            (l, r) = slot.main.process(l, r);
        }
        (l, r)
    }

    // Everything before the manifest's cue tap, gated by cue_active. Always
    // called so the cue units' state stays in sync with the main path; the
    // output is silenced rather than the processing skipped.
    #[inline]
    pub fn process_cue(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (mut l, mut r) = (l, r);
        for slot in &mut self.slots {
            if let Some(cue) = slot.cue.as_mut() {
                (l, r) = cue.process(l, r);
            }
        }
        if self.cue_active {
            (l, r)
        } else {
            (0.0, 0.0)
        }
    }
}

// main_pos (master callback, source of truth) and cue_pos (cue callback) advance
// independently from the same start, staying in sync within sub-ms drift.

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

    // Matches the floor set_playback_rate applies to playback_rate, so the
    // effective step (playback_rate * nudge_factor) can never reach zero.
    const NUDGE_FACTOR_MIN: f64 = 0.1;

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

    // Nudge as a percentage bend, floored so the effective step stays positive.
    // The UI clamps its own slider, but a hand-edited .bms or a MIDI mapping can
    // reach this directly, and a percent below -100 would drive playback
    // backwards through next_pos and read_at, which both assume forward motion.
    pub fn set_nudge_percent(&mut self, percent: f64) {
        self.nudge_factor = (1.0 + percent / 100.0).max(Self::NUDGE_FACTOR_MIN);
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

    // Main and cue step together because `next_pos` on the main path clears
    // `is_playing` and resets `cue_pos` at end of track, which the cue path has
    // to observe on the frame it happened.
    pub fn render_block(
        &mut self,
        strip: &mut ChannelStrip,
        frames: usize,
        targets: RenderTargets<'_>,
    ) -> (f32, f32) {
        let RenderTargets { main, cue } = targets;
        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;

        // Not gated on `cue_active`: the cue path is silenced at its output,
        // and its filter state has to keep tracking the main path.
        if !self.is_playing {
            if strip.settled() {
                return (0.0, 0.0);
            }
            strip.consume_settle(frames);
        } else {
            strip.restart_settle();
        }

        match (main, cue) {
            // One loop over both paths rather than one loop each, because
            // `main_tick` resets `cue_pos` at end of track. Pinned by
            // `end_of_track_inside_the_block_still_matches`.
            (Some(main), Some(cue)) => {
                for frame in 0..frames {
                    let (l, r) = self.main_tick();
                    let (ml, mr) = strip.process_main(l, r);
                    sum_l += ml.abs();
                    sum_r += mr.abs();
                    main[frame * 2] += ml;
                    main[frame * 2 + 1] += mr;

                    let (l, r) = self.cue_tick();
                    let (cl, cr) = strip.process_cue(l, r);
                    cue[frame * 2] += cl;
                    cue[frame * 2 + 1] += cr;
                }
            }
            (Some(main), None) => {
                for frame in 0..frames {
                    let (l, r) = self.main_tick();
                    let (ml, mr) = strip.process_main(l, r);
                    sum_l += ml.abs();
                    sum_r += mr.abs();
                    main[frame * 2] += ml;
                    main[frame * 2 + 1] += mr;
                }
            }
            (None, Some(cue)) => {
                for frame in 0..frames {
                    let (l, r) = self.cue_tick();
                    let (cl, cr) = strip.process_cue(l, r);
                    cue[frame * 2] += cl;
                    cue[frame * 2 + 1] += cr;
                }
            }
            (None, None) => {}
        }

        (sum_l, sum_r)
    }

    fn read_at(&self, pos: f64) -> (f32, f32) {
        if self.channels == 0 {
            return (0.0, 0.0);
        }
        // A negative pos would make interp_factor negative and extrapolate off
        // the front of the buffer, growing without bound the further back it
        // goes (a -44100 position yields samples ~2700x full scale).
        let pos = pos.max(0.0);
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

        // A degenerate loop (loop_end at or before cue_point) is treated as no
        // loop at all. Wrapping to cue_point instead would pin the playhead
        // there forever: every subsequent tick re-enters this branch, so the
        // deck never advances and never reaches the end-of-track check below.
        // Matches the guard compensate_late_start already applies.
        let loop_len = self.loop_end - self.cue_point;
        if self.loop_active && loop_len > 0.0 && new_pos >= self.loop_end {
            return self.cue_point + (new_pos - self.loop_end) % loop_len;
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

    // read_at used to extrapolate off the front of the buffer for a negative
    // position: interp_factor became the (large, negative) position itself, so
    // a -44100 read produced samples ~2700x full scale.
    #[test]
    fn read_at_clamps_negative_position() {
        let d = DeckState::loaded_for_testing(SR, 1.0);
        for pos in [-1.0f64, -400.0, -44_100.0] {
            let (l, r) = d.read_at(pos);
            assert!(l.abs() <= 1.0, "pos {pos} produced l={l}");
            assert!(r.abs() <= 1.0, "pos {pos} produced r={r}");
        }
        assert_eq!(d.read_at(-500.0), d.read_at(0.0));
    }

    #[test]
    fn set_nudge_percent_floors_the_factor() {
        let mut d = DeckState::loaded_for_testing(SR, 1.0);
        d.set_nudge_percent(-200.0);
        assert!(
            d.nudge_factor >= 0.1,
            "nudge_factor must stay positive, got {}",
            d.nudge_factor
        );
        d.set_nudge_percent(4.0);
        assert!((d.nudge_factor - 1.04).abs() < 1e-9);
        d.set_nudge_percent(-4.0);
        assert!((d.nudge_factor - 0.96).abs() < 1e-9);
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

    // A loop whose end is at or before the cue point used to wrap to cue_point
    // on every tick, pinning the playhead there forever with is_playing still
    // true and the end-of-track check never reached. session-core's sim_pos has
    // always guarded this (`loop_end > loop_start`), so the engine was the
    // divergent side.
    #[test]
    fn zero_length_loop_does_not_freeze_playback() {
        let mut d = deck_with_grid(10.0);
        d.cue_point = beat_frames();
        d.loop_end = beat_frames();
        d.loop_active = true;
        d.is_playing = true;
        d.main_pos = beat_frames();
        for _ in 0..1000 {
            d.main_tick();
        }
        assert!(
            d.main_pos > beat_frames(),
            "playhead must advance past a zero-length loop, stuck at {}",
            d.main_pos
        );
    }

    #[test]
    fn inverted_loop_does_not_freeze_playback() {
        let mut d = deck_with_grid(10.0);
        d.cue_point = beat_frames() * 2.0;
        d.loop_end = beat_frames();
        d.loop_active = true;
        d.is_playing = true;
        d.main_pos = beat_frames() * 2.0;
        for _ in 0..1000 {
            d.main_tick();
        }
        assert!(
            d.main_pos > beat_frames() * 2.0,
            "playhead must advance past an inverted loop, stuck at {}",
            d.main_pos
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

// The loops these replace run only inside a live cpal callback, so nothing else
// in the suite exercises them.
#[cfg(test)]
mod render_block_equivalence {
    use super::*;

    const SR: u32 = 44100;
    const FRAMES: usize = 512;

    fn deck_and_strip(duration_secs: f64) -> (DeckState, ChannelStrip) {
        let mut deck = DeckState::loaded_for_testing(SR, duration_secs);
        deck.is_playing = true;
        let mut strip = ChannelStrip::new(SR as f32);
        strip.set_eq_band("low", 4.0);
        strip.set_eq_band("high", -6.0);
        strip.set_filter(-0.4);
        strip.set_filter_active(true);
        strip.set_gain(0.3);
        strip.cue_active = true;
        (deck, strip)
    }

    fn rendered(duration_secs: f64, targets: (bool, bool)) -> (Vec<f32>, Vec<f32>, (f32, f32)) {
        let (mut deck, mut strip) = deck_and_strip(duration_secs);
        let mut main = vec![0.0f32; FRAMES * 2];
        let mut cue = vec![0.0f32; FRAMES * 2];
        let level = deck.render_block(
            &mut strip,
            FRAMES,
            RenderTargets {
                main: targets.0.then_some(&mut main),
                cue: targets.1.then_some(&mut cue),
            },
        );
        (main, cue, level)
    }

    fn reference(duration_secs: f64, targets: (bool, bool)) -> (Vec<f32>, Vec<f32>, (f32, f32)) {
        let (mut deck, mut strip) = deck_and_strip(duration_secs);
        let mut main = vec![0.0f32; FRAMES * 2];
        let mut cue = vec![0.0f32; FRAMES * 2];
        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;
        for frame in 0..FRAMES {
            if targets.0 {
                let (l, r) = deck.main_tick();
                let (ml, mr) = strip.process_main(l, r);
                sum_l += ml.abs();
                sum_r += mr.abs();
                main[frame * 2] += ml;
                main[frame * 2 + 1] += mr;
            }
            if targets.1 {
                let (l, r) = deck.cue_tick();
                let (cl, cr) = strip.process_cue(l, r);
                cue[frame * 2] += cl;
                cue[frame * 2 + 1] += cr;
            }
        }
        (main, cue, (sum_l, sum_r))
    }

    fn assert_same(label: &str, duration_secs: f64, targets: (bool, bool)) {
        let (main, cue, level) = rendered(duration_secs, targets);
        let (want_main, want_cue, want_level) = reference(duration_secs, targets);
        assert_eq!(main, want_main, "{label}: main output differs");
        assert_eq!(cue, want_cue, "{label}: cue output differs");
        assert_eq!(level, want_level, "{label}: metering sum differs");
    }

    #[test]
    fn main_only_matches_the_loop_it_replaced() {
        assert_same("main only", 1.0, (true, false));
    }

    #[test]
    fn cue_only_matches_the_loop_it_replaced() {
        assert_same("cue only", 1.0, (false, true));
    }

    #[test]
    fn main_and_cue_together_match_the_interleaved_loop() {
        assert_same("both", 1.0, (true, true));
    }

    #[test]
    fn end_of_track_inside_the_block_still_matches() {
        let short = (FRAMES / 2) as f64 / SR as f64;
        assert_same("end of track mid-block", short, (true, true));

        let (_, cue, _) = rendered(short, (true, true));

        let (mut deck, mut strip) = deck_and_strip(short);
        let mut reordered = vec![0.0f32; FRAMES * 2];
        for _ in 0..FRAMES {
            let (l, r) = deck.main_tick();
            strip.process_main(l, r);
        }
        for frame in 0..FRAMES {
            let (l, r) = deck.cue_tick();
            let (cl, cr) = strip.process_cue(l, r);
            reordered[frame * 2] += cl;
            reordered[frame * 2 + 1] += cr;
        }

        assert_ne!(
            cue, reordered,
            "interleaving main and cue per frame must matter across end of track"
        );
        let peak = |slice: &[f32]| slice.iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
        assert!(
            peak(&cue) > peak(&reordered) * 10.0,
            "the reordered render loses the cue tail: {:e} vs {:e}",
            peak(&cue),
            peak(&reordered)
        );
    }

    #[test]
    fn a_path_that_was_not_requested_is_left_untouched() {
        let (main, cue, level) = rendered(1.0, (false, true));
        assert!(main.iter().all(|s| *s == 0.0), "main buffer was written");
        assert!(cue.iter().any(|s| *s != 0.0), "cue buffer was not written");
        assert_eq!(level, (0.0, 0.0), "cue-only render reported a main level");
    }
}

#[cfg(test)]
mod silence_early_out {
    use super::*;

    const SR: u32 = 44100;
    const FRAMES: usize = 512;

    fn stopped_pair() -> (DeckState, ChannelStrip) {
        let mut deck = DeckState::loaded_for_testing(SR, 1.0);
        deck.is_playing = false;
        (deck, ChannelStrip::new(SR as f32))
    }

    fn render(deck: &mut DeckState, strip: &mut ChannelStrip, blocks: usize) {
        let mut main = vec![0.0f32; FRAMES * 2];
        for _ in 0..blocks {
            main.fill(0.0);
            deck.render_block(
                strip,
                FRAMES,
                RenderTargets {
                    main: Some(&mut main),
                    cue: None,
                },
            );
        }
    }

    fn blocks_to_settle() -> usize {
        (SR as usize).div_ceil(FRAMES) + 1
    }

    #[test]
    fn a_stopped_deck_is_skipped_once_its_strip_has_settled() {
        let (mut deck, mut strip) = stopped_pair();
        render(&mut deck, &mut strip, blocks_to_settle());
        assert!(strip.settled(), "strip should have settled by now");
    }

    #[test]
    fn a_playing_deck_is_never_skipped() {
        let (mut deck, mut strip) = stopped_pair();
        render(&mut deck, &mut strip, blocks_to_settle());
        assert!(strip.settled());

        deck.is_playing = true;
        render(&mut deck, &mut strip, 1);
        assert!(!strip.settled(), "playing must restart the settle window");
    }

    #[test]
    fn a_fader_moved_while_stopped_still_converges() {
        let (mut deck, mut strip) = stopped_pair();
        render(&mut deck, &mut strip, blocks_to_settle());
        assert!(strip.settled(), "precondition: already skipping");

        strip.set_gain(0.25);
        assert!(!strip.settled(), "a fader move must restart processing");
        render(&mut deck, &mut strip, blocks_to_settle());

        let (out, _) = strip.process_main(1.0, 1.0);
        assert!(
            (out - 0.25).abs() < 1e-4,
            "gain stalled at {out} instead of converging to 0.25"
        );
    }

    #[test]
    fn a_filter_moved_while_stopped_still_converges() {
        let (mut deck, mut strip) = stopped_pair();
        render(&mut deck, &mut strip, blocks_to_settle());

        strip.set_filter(-0.8);
        strip.set_filter_active(true);
        assert!(!strip.settled());
        render(&mut deck, &mut strip, blocks_to_settle());

        // A knob frozen near 0 leaves the low pass wide open, so a tone well
        // above the swept cutoff coming through is the symptom.
        let mut sum = 0.0;
        for frame in 0..1000 {
            let phase = frame as f32 / SR as f32;
            let sample = (std::f32::consts::TAU * 10_000.0 * phase).sin();
            let (l, _) = strip.process_main(sample, sample);
            sum += l * l;
        }
        let rms = (sum / 1000.0).sqrt();
        assert!(rms < 0.05, "filter knob stalled mid-smoothing, rms {rms}");
    }

    #[test]
    fn a_skipped_block_leaves_the_buffer_untouched() {
        let (mut deck, mut strip) = stopped_pair();
        render(&mut deck, &mut strip, blocks_to_settle());
        assert!(strip.settled());

        let mut main = vec![0.25f32; FRAMES * 2];
        let level = deck.render_block(
            &mut strip,
            FRAMES,
            RenderTargets {
                main: Some(&mut main),
                cue: None,
            },
        );
        assert!(
            main.iter().all(|s| *s == 0.25),
            "a skipped block wrote into the mix buffer"
        );
        assert_eq!(level, (0.0, 0.0));
    }
}
