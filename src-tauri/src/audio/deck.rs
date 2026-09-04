use super::unit::{make_unit, AudioUnit};
use crate::audio::atomic_f32::AtomicF32;
use session_core::{MixerManifest, ParamScope};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

// Gain smoothing, filter smoothing and IIR decay all run per sample, so a stopped
// strip cannot be skipped until they settle. One second is ~20 tau of the slowest.
const SETTLE_SECONDS: f32 = 1.0;
// Matches the fader's own smoothing, so a cut feels as immediate as the fader.
const XFADER_SMOOTHING_TAU_SEC: f32 = 0.010;

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

/// The frame an event is stamped with. Only a locked deck or strip can mint one, so a
/// stamp can never come from the free-running clock, which names the next buffer while
/// the callback is still filling the current one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderFrame(u64);

impl RenderFrame {
    pub(crate) fn get(self) -> u64 {
        self.0
    }

    /// The one way to make a stamp without a lock, for the two events that belong to the
    /// recording rather than to a deck.
    pub(crate) fn from_master_clock(frame: u64) -> Self {
        Self(frame)
    }
}

pub struct ChannelStrip {
    manifest: &'static MixerManifest,
    slots: Vec<Slot>,
    fader_slot: usize,
    pub(crate) cue_active: bool,
    // The assign is an enum, so neither half is a manifest param. Both live here to give
    // the resolved gain one owner, since a missed re-resolve anywhere is audible.
    pub(crate) xfader_assign: session_core::XfaderAssign,
    xfader_position: f32,
    xfader_gain: f32,
    metered: (f32, f32),
    xfader_gain_target: f32,
    xfader_smooth_coeff: f32,
    pub(crate) next_render_frame: u64,
    level_l: Arc<AtomicF32>,
    level_r: Arc<AtomicF32>,
    settle_frames: usize,
    settle_window_frames: usize,
}

impl ChannelStrip {
    #[cfg(test)]
    pub fn new(sample_rate: f32) -> Self {
        Self::from_manifest(super::MIXER, sample_rate)
    }

    /// Panics on a manifest this build cannot realize. Callers resolve it first through
    /// `session_core::resolve_manifest`, which reports an unusable one to the user.
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

        // Units construct themselves at their descriptor defaults, so a strip is usable without
        // a reset pass. `constructed_units_match_the_manifest_defaults` holds those in step.
        let settle_window_frames = (sample_rate * SETTLE_SECONDS) as usize;
        Self {
            manifest,
            slots,
            fader_slot,
            cue_active: false,
            xfader_assign: session_core::XfaderAssign::Thru,
            xfader_position: 0.0,
            xfader_gain: 1.0,
            metered: (0.0, 0.0),
            xfader_gain_target: 1.0,
            xfader_smooth_coeff: 1.0 - (-1.0 / (sample_rate * XFADER_SMOOTHING_TAU_SEC)).exp(),
            next_render_frame: 0,
            level_l: Arc::new(AtomicF32::default()),
            level_r: Arc::new(AtomicF32::default()),
            settle_frames: settle_window_frames,
            settle_window_frames,
        }
    }

    /// Master scope, so the strip's own param loop never reaches it.
    pub(crate) fn set_fader_curve(&mut self, curve: session_core::FaderCurve) {
        self.restart_settle();
        self.slots[self.fader_slot].main.set_fader_curve(curve);
    }

    pub(crate) fn set_xfader_assign(&mut self, assign: session_core::XfaderAssign) {
        self.xfader_assign = assign;
        self.resolve_xfader();
        self.restart_settle();
    }

    pub(crate) fn set_xfader_position(&mut self, position: f32) {
        self.xfader_position = position.clamp(-1.0, 1.0);
        self.resolve_xfader();
        // A settled strip renders nothing, so without this the smoothing ramp would
        // not run until playback resumed and the first samples would use the old gain.
        self.restart_settle();
    }

    fn resolve_xfader(&mut self) {
        self.xfader_gain_target = self.xfader_assign.gain(f64::from(self.xfader_position)) as f32;
    }

    pub fn store_level(&self, l: f32, r: f32) {
        self.level_l.set(l);
        self.level_r.set(r);
    }

    pub fn get_level(&self) -> [f32; 2] {
        [self.level_l.get(), self.level_r.get()]
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

    pub(crate) fn render_frame(&self) -> RenderFrame {
        RenderFrame(self.next_render_frame)
    }

    pub(crate) fn set_next_render_frame(&mut self, buffer_end: u64) {
        self.next_render_frame = buffer_end;
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

    /// Both output paths, so the cue signal tracks the main one. An address the manifest
    /// does not describe is ignored, which lets a richer mixer's session replay the rest.
    pub fn set_param(&mut self, slot: &str, param: &str, value: f32) -> bool {
        let Some(descriptor) = self.manifest.descriptor(ParamScope::Deck, slot, param) else {
            return false;
        };
        let value = descriptor.clamp(f64::from(value)) as f32;
        let Some(entry) = self.slots.iter_mut().find(|entry| entry.name == slot) else {
            return false;
        };
        entry.main.set_param(param, value);
        if let Some(cue) = entry.cue.as_mut() {
            cue.set_param(param, value);
        }
        self.restart_settle();
        true
    }

    // Separate from the fader gain, and not cleared by reset(), because scrubbing
    // rebuilds strips from session state and an audition mute has to survive that.
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
        // The crossfader is master scope, so the strip loop above never reaches it. A throw left
        // over from performance mode would silence a deck for a whole session.
        self.set_xfader_assign(session_core::XfaderAssign::Thru);
        self.set_xfader_position(0.0);
        self.set_fader_curve(session_core::FaderCurve::default());
    }

    #[inline]
    /// The channel's own level, read before the crossfader. Hardware channel meters do
    /// not follow the throw, and the cue sheet's audibility test reads the fader instead.
    pub(crate) fn metered(&self) -> (f32, f32) {
        self.metered
    }

    pub fn process_main(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (mut l, mut r) = (l, r);
        for slot in &mut self.slots {
            (l, r) = slot.main.process(l, r);
        }
        // Kept pre-crossfader, because a channel meter reads the channel: on hardware it
        // does not drop when the throw moves away from it.
        self.metered = (l.abs(), r.abs());
        // Post-fader and after the cue tap, so a deck crossfaded away is still audible in
        // headphones. Smoothed on the fader's one-pole because a fast throw clicks otherwise.
        self.xfader_gain = super::unit::approach(
            self.xfader_gain,
            self.xfader_gain_target,
            self.xfader_smooth_coeff,
        );
        (l * self.xfader_gain, r * self.xfader_gain)
    }

    // Everything before the manifest's cue tap, gated by cue_active. Always called so the
    // cue units stay in sync with the main path, silencing the output over skipping work.
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

#[derive(Debug, PartialEq)]
pub enum CuePressOutcome {
    PreviewStarted,
    CueMoved { new_cue_point_sec: f64 },
    StoppedAtCue { cue_point_sec: f64 },
    NoTrack,
}

/// Shift is scaled where the ticks are logged, so a replay needs no shift state of its
/// own. It lengthens a scrub only, leaving a playing deck's bend as the hardware does.
pub fn logged_jog_ticks(ticks: f64, shift_held: bool, is_playing: bool) -> f64 {
    if shift_held && !is_playing {
        ticks * session_core::JOG_SHIFT_MULTIPLIER
    } else {
        ticks
    }
}

struct JogFilter {
    pending: f64,
    arrived: f64,
    filtered: f64,
    frames_since_step: f64,
}

impl JogFilter {
    // A callback is whatever length the driver feels like, and stepping the filter on it
    // let the buffer cadence change how far the wheel travelled.
    const STEP_FRAMES: f64 = 128.0;

    fn idle(&self) -> bool {
        self.arrived == 0.0 && self.filtered == 0.0
    }

    fn deposit(&mut self, ticks: f64) {
        self.arrived += ticks;
    }

    fn queue(&mut self, ticks: f64) {
        self.pending += ticks;
    }

    fn drain_pending(&mut self) {
        self.arrived += std::mem::take(&mut self.pending);
    }

    /// So the first tick after an idle spell steps the grid rather than waiting out its phase.
    fn rearm(&mut self) {
        self.frames_since_step = Self::STEP_FRAMES;
    }

    /// The travel in frames when this frame completed a grid step, else `None`.
    fn advance_one_frame(&mut self, alpha: f64, frames_per_tick: f64) -> Option<f64> {
        let travel = (self.frames_since_step >= Self::STEP_FRAMES).then(|| {
            self.frames_since_step -= Self::STEP_FRAMES;
            self.step(alpha, frames_per_tick)
        });
        self.frames_since_step += 1.0;
        travel
    }

    fn step(&mut self, alpha: f64, frames_per_tick: f64) -> f64 {
        let arrived = std::mem::take(&mut self.arrived);
        self.filtered += (arrived - self.filtered) * alpha;
        if self.filtered.abs() < f64::EPSILON {
            self.filtered = 0.0;
        }
        self.filtered * frames_per_tick
    }
}

impl Default for JogFilter {
    fn default() -> Self {
        Self {
            pending: 0.0,
            arrived: 0.0,
            filtered: 0.0,
            frames_since_step: Self::STEP_FRAMES,
        }
    }
}

/// Filled in after the track loads and read only by the waveform drawing, never by
/// the transport.
#[derive(Clone)]
pub struct SpectralBands {
    pub bass: Arc<Vec<f32>>,
    pub mid: Arc<Vec<f32>>,
    pub high: Arc<Vec<f32>>,
    pub bass_rms: f32,
    pub mid_rms: f32,
    pub high_rms: f32,
    pub source_rate: u32,
}

impl SpectralBands {
    pub fn frames(&self) -> usize {
        self.bass.len()
    }
}

impl Default for SpectralBands {
    fn default() -> Self {
        Self {
            bass: Arc::new(Vec::new()),
            mid: Arc::new(Vec::new()),
            high: Arc::new(Vec::new()),
            bass_rms: 1.0,
            mid_rms: 1.0,
            high_rms: 1.0,
            source_rate: 0,
        }
    }
}

/// What a scrub restores onto a deck, so the caller names the values rather than
/// reaching into the fields one at a time.
pub(crate) struct DeckRestore {
    pub position: f64,
    pub cue_point: f64,
    pub loop_active: bool,
    pub loop_end: f64,
    pub playback_rate: f64,
    pub jog_hold_factor: f64,
    pub bpm: Option<f64>,
    pub beat_offset_frames: f64,
}

pub struct Deck {
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
    pub(crate) jog_hold_factor: f64, // 1 + nudge_percent/100
    jog: JogFilter,
    jog_bend: f64,
    pub(crate) next_render_frame: u64,
    pub(crate) jog_rotation_speed: session_core::JogRotationSpeed,
    pub(crate) jog_shift: bool,
    pub(crate) quantize: bool,

    pub(crate) bands: SpectralBands,
    /// Grows while the track is being analysed, so a drawer can paint what has arrived.
    pub(crate) dense_points: Vec<f32>,
    load_id: u64,

    // Set to true by the audio thread when the track reaches its natural end.
    // The monitoring task in lib.rs polls this and emits a "track-ended" event.
    pub(crate) just_ended: Arc<AtomicBool>,
}

impl Deck {
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
            jog_hold_factor: 1.0,
            jog: JogFilter::default(),
            jog_bend: 0.0,
            next_render_frame: 0,
            jog_rotation_speed: session_core::JogRotationSpeed::Rpm33,
            jog_shift: false,
            quantize: true,
            bands: SpectralBands::default(),
            dense_points: Vec::new(),
            load_id: 0,
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

    /// The one place a track's samples reach a deck. Three copies of this had drifted.
    pub(crate) fn load(
        &mut self,
        path: &str,
        samples: Arc<Vec<f32>>,
        channels: usize,
        sample_rate: u32,
    ) {
        let total_frames = samples.len() / channels;
        self.reset();
        self.samples = samples;
        self.channels = channels;
        self.device_sample_rate = sample_rate;
        self.total_frames = total_frames;
        self.duration = total_frames as f64 / f64::from(sample_rate);
        self.loaded_path = Some(path.to_string());
        self.bands = SpectralBands::default();
    }

    /// Where the deck sits when a track opens: the playhead and the cue both at `frames`.
    pub(crate) fn open_at(&mut self, frames: f64) {
        self.main_pos = frames;
        self.cue_pos = frames;
        self.cue_point = frames;
    }

    /// Both playheads together, which is what every seek and rewind wants.
    pub(crate) fn seek_both(&mut self, frames: f64) {
        self.main_pos = frames;
        self.cue_pos = frames;
    }

    // Only these two write it: a load that lands after a newer one started must leave the
    // newer one's reduction alone, and `load` runs through `reset`.
    pub(crate) fn begin_load(&mut self, load_id: u64) {
        self.load_id = load_id;
    }

    pub(crate) fn end_load(&mut self) {
        self.load_id = 0;
    }

    pub(crate) fn holds_load(&self, load_id: u64) -> bool {
        self.load_id == load_id
    }

    pub(crate) fn set_bands(&mut self, load_id: u64, bands: SpectralBands) {
        if !self.holds_load(load_id) {
            return;
        }
        self.bands = bands;
    }

    pub(crate) fn reset_dense_points(&mut self, load_id: u64, total_points: usize) {
        if !self.holds_load(load_id) {
            return;
        }
        self.dense_points = Vec::with_capacity(total_points * 4);
    }

    pub(crate) fn push_dense_points(&mut self, load_id: u64, points: &[f32]) {
        if !self.holds_load(load_id) {
            return;
        }
        self.dense_points.extend_from_slice(points);
    }

    /// The transport state a `deck_snapshot` restores, clamped to the loaded track.
    pub(crate) fn restore(&mut self, snapshot: DeckRestore) {
        let limit = self.total_frames as f64;
        self.seek_both(snapshot.position.min(limit));
        self.cue_point = snapshot.cue_point.min(limit);
        self.loop_active = snapshot.loop_active;
        self.loop_end = snapshot.loop_end.min(limit);
        self.playback_rate = snapshot.playback_rate;
        self.jog_hold_factor = snapshot.jog_hold_factor;
        self.bpm = snapshot.bpm;
        self.beat_offset_frames = snapshot.beat_offset_frames;
        self.is_playing = false;
        self.is_cueing = false;
    }

    pub(crate) fn stop(&mut self) {
        self.is_playing = false;
    }

    pub(crate) fn set_quantize(&mut self, quantize: bool) {
        self.quantize = quantize;
    }

    pub(crate) fn reset(&mut self) {
        self.eject();
        self.playback_rate = 1.0;
        self.jog_hold_factor = 1.0;
        self.jog = JogFilter::default();
        self.jog_bend = 0.0;
    }

    /// Ticks arriving after this point are consumed by the buffer starting at `buffer_end`.
    /// Called under the same lock as `consume_jog`, which is what makes the answer exact.
    pub(crate) fn render_frame(&self) -> RenderFrame {
        RenderFrame(self.next_render_frame)
    }

    pub(crate) fn set_next_render_frame(&mut self, buffer_end: u64) {
        self.next_render_frame = buffer_end;
    }

    // 50 frames at 44100 Hz ≈ 1.1 ms. Matches the frontend's 0.001 s tolerance.
    const CUE_THRESHOLD_FRAMES: f64 = 50.0;

    /// Reads state only. Which of the four things a press means depends on the playhead,
    /// so it is resolved here and applied as the command it resolved to.
    pub fn resolve_cue_press(&self) -> CuePressOutcome {
        let sr = self.device_sample_rate as f64;
        if self.total_frames == 0 {
            return CuePressOutcome::NoTrack;
        }
        if self.is_cueing {
            return CuePressOutcome::PreviewStarted;
        }
        if self.is_playing {
            return CuePressOutcome::StoppedAtCue {
                cue_point_sec: self.cue_point / sr,
            };
        }
        if (self.main_pos - self.cue_point).abs() <= Self::CUE_THRESHOLD_FRAMES {
            return CuePressOutcome::PreviewStarted;
        }
        CuePressOutcome::CueMoved {
            new_cue_point_sec: self.main_pos / sr,
        }
    }

    pub fn press_cue(&mut self) -> CuePressOutcome {
        let outcome = self.resolve_cue_press();
        match outcome {
            CuePressOutcome::NoTrack => {}
            CuePressOutcome::StoppedAtCue { .. } => {
                self.is_playing = false;
                self.main_pos = self.cue_point;
                self.cue_pos = self.cue_point;
            }
            CuePressOutcome::PreviewStarted if !self.is_cueing => {
                self.is_playing = true;
                self.is_cueing = true;
                self.main_pos = self.cue_point;
                self.cue_pos = self.cue_point;
            }
            CuePressOutcome::PreviewStarted => {}
            CuePressOutcome::CueMoved { .. } => {
                self.cue_point = self.main_pos;
            }
        }
        outcome
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

    /// A held control's release never arrives once the surface stops being listened to,
    /// and nothing else clears it, so leaving performance mode ends the hold instead.
    pub(crate) fn release_held_controls(&mut self) {
        self.jog_shift = false;
        self.release_cue();
    }

    pub fn position_sec(&self) -> f64 {
        if self.device_sample_rate == 0 {
            return 0.0;
        }
        self.main_pos / self.device_sample_rate as f64
    }

    pub fn set_jog_rotation_speed(&mut self, speed: session_core::JogRotationSpeed) {
        self.jog_rotation_speed = speed;
    }

    pub(crate) fn set_jog_shift(&mut self, held: bool) {
        self.jog_shift = held;
    }

    fn jog_frames_per_tick(&self) -> f64 {
        self.jog_rotation_speed
            .frames_per_tick(self.device_sample_rate as f64)
    }

    fn jog_filter_alpha(&self) -> f64 {
        if self.device_sample_rate == 0 {
            return 1.0;
        }
        let step_seconds = JogFilter::STEP_FRAMES / self.device_sample_rate as f64;
        1.0 - (-step_seconds / session_core::JOG_FILTER_TAU_SEC).exp()
    }

    fn advance_jog_grid_one_frame(&mut self) {
        // Per frame, not per callback: a block is whatever length the driver chose, and the
        // same wheel movement has to bend playback by the same amount at every buffer size.
        self.jog.drain_pending();
        if self.jog.idle() && self.jog_bend == 0.0 {
            return;
        }
        if let Some(travel) = self
            .jog
            .advance_one_frame(self.jog_filter_alpha(), self.jog_frames_per_tick())
        {
            self.jog_bend = travel / (JogFilter::STEP_FRAMES * session_core::JOG_PAUSED_MULTIPLIER);
        }
    }

    pub(crate) fn deposit_jog(&mut self, ticks: f64) {
        self.jog.deposit(ticks);
    }

    #[cfg(test)]
    pub(crate) fn jog_filtered(&self) -> f64 {
        self.jog.filtered
    }

    /// Only `consume_jog` moves these onto the grid, under the lock the callback takes.
    pub(crate) fn queue_jog(&mut self, ticks: f64) {
        self.jog.queue(ticks);
    }

    fn advance_paused_jog_one_frame(&mut self) {
        if self.jog.idle() {
            self.jog.rearm();
            return;
        }
        if let Some(travel) = self
            .jog
            .advance_one_frame(self.jog_filter_alpha(), self.jog_frames_per_tick())
        {
            self.main_pos = (self.main_pos + travel).clamp(0.0, self.total_frames as f64);
        }
    }

    pub(crate) fn consume_jog(&mut self, frames: usize) {
        if self.is_playing {
            return;
        }
        self.jog_bend = 0.0;
        for _ in 0..frames {
            self.jog.drain_pending();
            self.advance_paused_jog_one_frame();
        }
        self.cue_pos = self.main_pos;
        if self.outside_loop(self.main_pos) {
            self.loop_active = false;
        }
    }

    // Floored so the effective step stays positive. A hand-edited .bms or MIDI mapping can
    // reach this directly, and a percent below -100 drives next_pos and read_at backwards.
    pub fn set_nudge_percent(&mut self, percent: f64) {
        self.jog_hold_factor = (1.0 + percent / 100.0).max(session_core::JOG_FACTOR_MIN);
    }

    #[inline]
    pub fn main_tick(&mut self) -> (f32, f32) {
        if !self.is_playing || self.samples.is_empty() {
            return (0.0, 0.0);
        }
        self.advance_jog_grid_one_frame();
        let (l, r) = self.read_at(self.main_pos);
        self.main_pos = self.next_pos(self.main_pos, true);
        (l, r)
    }

    // cue_pos always advances while playing so it stays in sync with main_pos
    // regardless of cue_active.
    #[inline]
    pub fn cue_tick(&mut self) -> (f32, f32) {
        if !self.is_playing || self.samples.is_empty() {
            return (0.0, 0.0);
        }
        let (l, r) = self.read_at(self.cue_pos);
        self.cue_pos = self.next_pos(self.cue_pos, false);
        (l, r)
    }

    // Main and cue step together because `next_pos` clears `is_playing` and resets `cue_pos`
    // at end of track, which the cue path has to observe on the frame it happened.
    pub fn render_block(
        &mut self,
        strip: &mut ChannelStrip,
        frames: usize,
        targets: RenderTargets<'_>,
    ) -> (f32, f32) {
        let RenderTargets { main, cue } = targets;
        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;
        // A separate cue device gives this deck two callbacks per period. Anything consumed once
        // per period belongs to the main one, or a paused scrub travels double distance.
        let owns_period = main.is_some();

        // Ahead of the settled check below, because scrubbing a paused deck is
        // the one thing that has to happen on a block that renders no audio.
        if owns_period {
            self.consume_jog(frames);
        }

        // Not gated on `cue_active`: the cue path is silenced at its output,
        // and its filter state has to keep tracking the main path.
        if !self.is_playing {
            if strip.settled() {
                return (0.0, 0.0);
            }
            if owns_period {
                strip.consume_settle(frames);
            }
        } else {
            strip.restart_settle();
        }

        match (main, cue) {
            // One loop over both paths, because `main_tick` resets `cue_pos` at end of track.
            // Pinned by `end_of_track_inside_the_block_still_matches`.
            (Some(main), Some(cue)) => {
                for frame in 0..frames {
                    let (l, r) = self.main_tick();
                    let (ml, mr) = strip.process_main(l, r);
                    let (metered_l, metered_r) = strip.metered();
                    sum_l += metered_l;
                    sum_r += metered_r;
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
                    let (metered_l, metered_r) = strip.metered();
                    sum_l += metered_l;
                    sum_r += metered_r;
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
        // A negative pos makes interp_factor negative and extrapolates off the front of the
        // buffer, growing without bound: a -44100 position yields samples ~2700x full scale.
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

    /// A degenerate region counts as no region: wrapping to `cue_point` would re-enter
    /// every tick, pinning the playhead so it never reaches the end of the track.
    fn wrap_into_loop(&self, pos: f64) -> Option<f64> {
        let length = self.loop_end - self.cue_point;
        if !self.loop_active || length <= 0.0 || pos < self.loop_end {
            return None;
        }
        Some(self.cue_point + (pos - self.loop_end) % length)
    }

    /// A playhead scrubbed or seeked out of the region would otherwise be wrapped
    /// straight back into it on the first frame of playback.
    pub(crate) fn outside_loop(&self, pos: f64) -> bool {
        self.loop_active && (pos < self.cue_point || pos >= self.loop_end)
    }

    fn next_pos(&mut self, pos: f64, is_main: bool) -> f64 {
        // One bend for both inputs. A held nudge is the constant case of a wheel velocity, so
        // with the wheel idle this reduces to `jog_hold_factor` and sessions replay frame for frame.
        let wheel = self.jog_bend;
        let factor = (self.jog_hold_factor + wheel).max(session_core::JOG_FACTOR_MIN);
        let step = self.playback_rate * factor;
        let new_pos = pos + step;

        if let Some(wrapped) = self.wrap_into_loop(new_pos) {
            return wrapped;
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

    /// A deck started or repositioned a fraction of a buffer late would otherwise sit
    /// behind the decks already playing.
    pub(crate) fn compensate_late_start(&mut self, overshoot_frames: f64) {
        if !self.is_playing || overshoot_frames <= 0.0 {
            return;
        }
        let raw = self.main_pos + overshoot_frames * self.playback_rate * self.jog_hold_factor;
        let mut pos = self.wrap_into_loop(raw).unwrap_or(raw);
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
    const BLOCK: usize = 512;

    impl Deck {
        // Creates a Deck loaded with a 440 Hz sine wave. No audio hardware.
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
            let mut d = Deck::empty(device_sample_rate);
            d.samples = Arc::new(samples);
            d.total_frames = total_frames;
            d.duration = duration_secs;
            d
        }
    }

    #[test]
    fn deck_stops_at_natural_end_of_track() {
        let mut d = Deck::loaded_for_testing(SR, 1.0);
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
        let mut d = Deck::loaded_for_testing(SR, 5.0);
        d.is_playing = false;
        let (l, r) = d.main_tick();
        assert_eq!(l, 0.0);
        assert_eq!(r, 0.0);
    }

    #[test]
    fn deck_position_advances_while_playing() {
        let mut d = Deck::loaded_for_testing(SR, 5.0);
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

    #[test]
    fn a_scrub_moves_the_same_distance_with_a_separate_cue_device() {
        fn scrubbed(separate_cue: bool) -> f64 {
            let mut deck = Deck::loaded_for_testing(SR, 10.0);
            let mut strip = ChannelStrip::new(SR as f32);
            deck.is_playing = false;
            deck.main_pos = 44_100.0;
            deck.queue_jog(20.0);
            let mut main = vec![0.0f32; 512 * 2];
            let mut cue = vec![0.0f32; 512 * 2];

            deck.render_block(
                &mut strip,
                512,
                RenderTargets {
                    main: Some(&mut main),
                    cue: if separate_cue { None } else { Some(&mut cue) },
                },
            );
            if separate_cue {
                deck.render_block(
                    &mut strip,
                    512,
                    RenderTargets {
                        main: None,
                        cue: Some(&mut cue),
                    },
                );
            }
            deck.main_pos
        }

        let one_stream = scrubbed(false);
        assert!(one_stream > 44_100.0, "the wheel should have scrubbed");
        assert!((scrubbed(true) - one_stream).abs() < 1e-9);
    }

    #[test]
    fn the_cue_path_keeps_pace_with_the_main_path_whether_cue_is_on_or_off() {
        for cue_active in [false, true] {
            let mut deck = Deck::loaded_for_testing(SR, 5.0);
            let mut strip = ChannelStrip::new(SR as f32);
            strip.cue_active = cue_active;
            deck.is_playing = true;
            let mut main = vec![0.0f32; 512 * 2];
            let mut cue = vec![0.0f32; 512 * 2];

            deck.render_block(
                &mut strip,
                512,
                RenderTargets {
                    main: Some(&mut main),
                    cue: Some(&mut cue),
                },
            );

            assert_eq!(deck.cue_pos, deck.main_pos, "cue_active = {cue_active}");
            assert!(deck.cue_pos > 0.0, "cue_active = {cue_active}");
        }
    }

    #[test]
    fn read_at_clamps_negative_position() {
        let d = Deck::loaded_for_testing(SR, 1.0);
        for pos in [-1.0f64, -400.0, -44_100.0] {
            let (l, r) = d.read_at(pos);
            assert!(l.abs() <= 1.0, "pos {pos} produced l={l}");
            assert!(r.abs() <= 1.0, "pos {pos} produced r={r}");
        }
        assert_eq!(d.read_at(-500.0), d.read_at(0.0));
    }

    #[test]
    fn set_nudge_percent_floors_the_factor() {
        let mut d = Deck::loaded_for_testing(SR, 1.0);
        d.set_nudge_percent(-200.0);
        assert!(
            d.jog_hold_factor >= 0.1,
            "jog_hold_factor must stay positive, got {}",
            d.jog_hold_factor
        );
        d.set_nudge_percent(4.0);
        assert!((d.jog_hold_factor - 1.04).abs() < 1e-9);
        d.set_nudge_percent(-4.0);
        assert!((d.jog_hold_factor - 0.96).abs() < 1e-9);
    }

    #[test]
    fn a_held_nudge_advances_exactly_as_it_did_before_the_wheel_existed() {
        for (rate, percent) in [(1.0, 4.0), (1.0, -4.0), (0.94, 8.0), (1.08, -2.0)] {
            let mut deck = Deck::loaded_for_testing(SR, 1.0);
            deck.playback_rate = rate;
            deck.set_nudge_percent(percent);
            deck.is_playing = true;

            let expected = rate * (1.0 + percent / 100.0);
            for _ in 0..8 {
                deck.consume_jog(BLOCK);
                let before = deck.main_pos;
                let after = deck.next_pos(before, false);
                assert!(
                    (after - before - expected).abs() < 1e-12,
                    "rate {rate} percent {percent}: stepped {}, expected {expected}",
                    after - before
                );
            }
        }
    }

    #[test]
    fn a_decks_audio_does_not_depend_on_how_the_caller_chunks_the_frames() {
        const TOTAL: usize = 4096;

        let render = |sizes: &[usize]| -> Vec<f32> {
            let mut deck = Deck::loaded_for_testing(SR, 10.0);
            let mut strip = ChannelStrip::from_manifest(&session_core::CLASSIC_3BAND_V2, SR as f32);
            deck.is_playing = true;
            deck.main_pos = 50_000.0;
            deck.queue_jog(40.0);

            let mut out = Vec::with_capacity(TOTAL * 2);
            let mut frame = 0;
            let mut block = 0;
            while frame < TOTAL {
                let size = sizes[block % sizes.len()].min(TOTAL - frame);
                let mut main = vec![0.0f32; size * 2];
                deck.render_block(
                    &mut strip,
                    size,
                    RenderTargets {
                        main: Some(&mut main),
                        cue: None,
                    },
                );
                out.extend_from_slice(&main);
                frame += size;
                block += 1;
            }
            out
        };

        let steady = render(&[128]);
        for sizes in [&[117usize, 118, 118, 117, 118][..], &[64][..], &[512][..]] {
            let chunked = render(sizes);
            let worst = steady
                .iter()
                .zip(&chunked)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(
                worst == 0.0,
                "chunking {sizes:?} moved the audio by {worst}, so the driver's block length is audible"
            );
        }
    }

    #[test]
    fn a_paused_scrubs_total_travel_is_its_tick_count_whatever_the_block_size() {
        let schedules: [(&str, fn(usize) -> usize); 6] = [
            ("64", |_| 64),
            ("128", |_| 128),
            ("512", |_| 512),
            ("1024", |_| 1024),
            ("117/118 alternating", |block| {
                117 + usize::from(block % 5 < 3)
            }),
            ("ragged", |block| [61, 512, 128, 7, 1024, 199][block % 6]),
        ];

        for (label, block_frames) in schedules {
            for ticks in [1.0, 6.0, -13.0, 400.0] {
                let mut deck = Deck::loaded_for_testing(SR, 10.0);
                deck.main_pos = 100_000.0;
                let start = deck.main_pos;
                let expected = ticks * deck.jog_frames_per_tick();

                deck.queue_jog(ticks);
                for block in 0..4096 {
                    deck.consume_jog(block_frames(block));
                }

                let travelled = deck.main_pos - start;
                assert!(
                    (travelled - expected).abs() < 1e-6,
                    "block {label} ticks {ticks}: travelled {travelled}, expected {expected}"
                );
            }
        }
    }

    #[test]
    fn a_playing_bends_total_travel_is_the_paused_travel_over_the_multiplier() {
        const BLOCK_FRAMES: usize = 512;
        // Three seconds against the wheel filter's 40 ms tau, so the tail is long settled.
        const BLOCKS: usize = 256;
        for ticks in [6.0, -6.0, 40.0] {
            let mut deck = Deck::loaded_for_testing(SR, 10.0);
            deck.is_playing = true;
            deck.playback_rate = 1.0;
            deck.main_pos = 100_000.0;
            let start = deck.main_pos;
            let expected = ticks * deck.jog_frames_per_tick() / session_core::JOG_PAUSED_MULTIPLIER
                + (BLOCKS * BLOCK_FRAMES) as f64;

            deck.queue_jog(ticks);
            for _ in 0..BLOCKS {
                deck.consume_jog(BLOCK_FRAMES);
                for _ in 0..BLOCK_FRAMES {
                    deck.main_tick();
                }
            }

            let travelled = deck.main_pos - start;
            assert!(
                (travelled - expected).abs() < 1e-3,
                "ticks {ticks}: travelled {travelled}, expected {expected}"
            );
        }
    }

    fn looping_deck_at(pos: f64) -> Deck {
        let mut deck = Deck::loaded_for_testing(SR, 600.0);
        deck.cue_point = 100_000.0;
        deck.loop_end = 200_000.0;
        deck.loop_active = true;
        deck.main_pos = pos;
        deck
    }

    fn settle_jog(deck: &mut Deck, ticks: f64) {
        deck.queue_jog(ticks);
        for _ in 0..4096 {
            deck.consume_jog(512);
        }
    }

    #[test]
    fn scrubbing_a_paused_deck_out_of_its_loop_disarms_it() {
        let mut deck = looping_deck_at(150_000.0);

        settle_jog(&mut deck, 2000.0);

        assert!(deck.main_pos > deck.loop_end);
        assert!(!deck.loop_active);
    }

    #[test]
    fn scrubbing_a_paused_deck_backwards_past_the_loop_in_disarms_it() {
        let mut deck = looping_deck_at(150_000.0);

        settle_jog(&mut deck, -2000.0);

        assert!(deck.main_pos < deck.cue_point);
        assert!(!deck.loop_active);
    }

    #[test]
    fn scrubbing_around_inside_the_loop_keeps_it_armed() {
        let mut deck = looping_deck_at(150_000.0);

        settle_jog(&mut deck, 100.0);

        assert!(deck.main_pos > 150_000.0 && deck.main_pos < deck.loop_end);
        assert!(deck.loop_active);
    }

    #[test]
    fn an_untouched_wheel_leaves_a_paused_deck_where_it_was() {
        let mut deck = Deck::loaded_for_testing(SR, 1.0);
        deck.main_pos = 1000.0;
        for _ in 0..64 {
            deck.consume_jog(BLOCK);
        }
        assert_eq!(deck.main_pos, 1000.0);
    }

    #[test]
    fn a_released_wheel_rings_down_to_no_bend() {
        let mut deck = Deck::loaded_for_testing(SR, 30.0);
        deck.is_playing = true;
        for _ in 0..8 {
            deck.queue_jog(20.0);
            deck.consume_jog(BLOCK);
            for _ in 0..BLOCK {
                deck.main_tick();
            }
        }
        let bent = deck.next_pos(0.0, false);
        assert!(bent > 1.0, "a turned wheel should bend forward, got {bent}");

        for _ in 0..400 {
            deck.consume_jog(BLOCK);
            for _ in 0..BLOCK {
                deck.main_tick();
            }
        }
        assert_eq!(deck.next_pos(0.0, false), 1.0);
    }

    #[test]
    fn a_turned_wheel_scrubs_a_paused_deck_both_ways() {
        let mut deck = Deck::loaded_for_testing(SR, 1.0);
        deck.main_pos = 20_000.0;

        deck.queue_jog(50.0);
        deck.consume_jog(BLOCK);
        assert!(deck.main_pos > 20_000.0);
        assert_eq!(deck.cue_pos, deck.main_pos);

        let forward = deck.main_pos;
        deck.queue_jog(-200.0);
        deck.consume_jog(BLOCK);
        assert!(deck.main_pos < forward);
    }

    #[test]
    fn the_wheel_settles_over_the_same_wall_clock_time_at_any_buffer_size() {
        const TICKS_PER_FRAME: f64 = 0.05;
        const TURN_MS: f64 = 120.0;
        const SETTLE_SLACK: f64 = 0.02;
        let settled_fraction = |frames: usize| {
            let mut deck = Deck::loaded_for_testing(SR, 1.0);
            deck.is_playing = true;
            let blocks = (TURN_MS / 1000.0 * f64::from(SR) / frames as f64).round() as usize;
            for _ in 0..blocks {
                deck.queue_jog(TICKS_PER_FRAME * frames as f64);
                deck.consume_jog(frames);
            }
            deck.jog_filtered() / (TICKS_PER_FRAME * JogFilter::STEP_FRAMES)
        };
        assert!((settled_fraction(256) - settled_fraction(1024)).abs() < SETTLE_SLACK);
        assert!((settled_fraction(128) - settled_fraction(1024)).abs() < SETTLE_SLACK);
        assert!((settled_fraction(117) - settled_fraction(1024)).abs() < SETTLE_SLACK);
    }

    #[test]
    fn the_same_hand_speed_bends_the_same_at_any_buffer_size() {
        const TICKS_PER_FRAME: f64 = 0.05;
        let bend = |frames: usize| {
            let mut deck = Deck::loaded_for_testing(SR, 1.0);
            deck.is_playing = true;
            for _ in 0..200 {
                deck.queue_jog(TICKS_PER_FRAME * frames as f64);
                deck.consume_jog(frames);
            }
            deck.next_pos(0.0, false)
        };
        assert!((bend(256) - bend(1024)).abs() < 1e-9);
    }

    #[test]
    fn one_revolution_covers_a_fixed_ratio_less_audio_at_the_faster_speed() {
        let travel = |speed| {
            let mut deck = Deck::loaded_for_testing(SR, 1.0);
            deck.set_jog_rotation_speed(speed);
            deck.main_pos = 20_000.0;
            deck.queue_jog(50.0);
            deck.consume_jog(BLOCK);
            deck.main_pos - 20_000.0
        };
        let slow = travel(session_core::JogRotationSpeed::Rpm33);
        let fast = travel(session_core::JogRotationSpeed::Rpm45);
        assert!(
            (slow / fast - session_core::JogRotationSpeed::Rpm45.scrub_scale().recip()).abs()
                < 1e-12
        );
    }

    #[test]
    fn shift_doubles_the_scrub_and_leaves_the_bend_alone() {
        assert_eq!(logged_jog_ticks(50.0, true, false), 100.0);
        assert_eq!(logged_jog_ticks(50.0, false, false), 50.0);
        assert_eq!(logged_jog_ticks(50.0, true, true), 50.0);

        let scrub = |held| {
            let mut deck = Deck::loaded_for_testing(SR, 1.0);
            deck.main_pos = 20_000.0;
            deck.queue_jog(logged_jog_ticks(50.0, held, false));
            deck.consume_jog(BLOCK);
            deck.main_pos - 20_000.0
        };
        assert!((scrub(true) / scrub(false) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn releasing_held_controls_ends_a_cue_preview_and_drops_shift() {
        let mut deck = Deck::loaded_for_testing(SR, 10.0);
        deck.set_jog_shift(true);
        deck.cue_point = 20_000.0;
        deck.main_pos = deck.cue_point;
        deck.press_cue();
        assert!(deck.is_cueing, "the preview has to be running to be ended");
        deck.main_pos = deck.cue_point + 800.0;

        deck.release_held_controls();

        assert!(!deck.jog_shift);
        assert!(!deck.is_cueing);
        assert!(!deck.is_playing);
        assert!((deck.main_pos - deck.cue_point).abs() < 1.0);
    }

    #[test]
    fn releasing_held_controls_leaves_normal_playback_alone() {
        let mut deck = Deck::loaded_for_testing(SR, 10.0);
        deck.is_playing = true;
        deck.main_pos = 30_000.0;

        deck.release_held_controls();

        assert!(deck.is_playing);
        assert!((deck.main_pos - 30_000.0).abs() < 1.0);
    }

    #[test]
    fn shift_survives_a_track_change() {
        let mut deck = Deck::loaded_for_testing(SR, 1.0);
        deck.set_jog_shift(true);
        deck.reset();
        assert!(deck.jog_shift);
    }

    #[test]
    fn the_rotation_speed_survives_a_track_change() {
        let mut deck = Deck::loaded_for_testing(SR, 1.0);
        deck.set_jog_rotation_speed(session_core::JogRotationSpeed::Rpm45);
        deck.reset();
        assert_eq!(
            deck.jog_rotation_speed,
            session_core::JogRotationSpeed::Rpm45
        );
    }

    #[test]
    fn scrubbing_cannot_leave_the_track() {
        let mut deck = Deck::loaded_for_testing(SR, 1.0);
        deck.main_pos = 10.0;
        for _ in 0..200 {
            deck.queue_jog(-500.0);
            deck.consume_jog(BLOCK);
        }
        assert!(deck.main_pos >= 0.0);

        for _ in 0..2000 {
            deck.queue_jog(500.0);
            deck.consume_jog(BLOCK);
        }
        assert!(deck.main_pos <= deck.total_frames as f64);
    }

    #[test]
    fn a_param_outside_the_descriptors_range_lands_on_the_range() {
        let mut strip = ChannelStrip::new(48000.0);

        strip.set_param("eq", "low", 60.0);
        assert_eq!(
            strip.param("eq", "low"),
            Some(session_core::EQ_MAX_DB as f32)
        );

        strip.set_param("eq", "high", -200.0);
        assert_eq!(
            strip.param("eq", "high"),
            Some(session_core::EQ_MIN_DB as f32)
        );

        strip.set_param("filter", "value", 4.0);
        assert_eq!(strip.param("filter", "value"), Some(1.0));
    }

    #[test]
    fn channel_strip_gain_does_not_jump_on_change() {
        let mut strip = ChannelStrip::new(48000.0);
        strip.set_param("fader", "gain", 0.0);
        let (l, _) = strip.process_main(1.0, 1.0);
        assert!(l > 0.5, "expected gain near 1.0 on first sample, got {}", l);
    }

    #[test]
    fn channel_strip_gain_converges_to_target() {
        let mut strip = ChannelStrip::new(48000.0);
        strip.set_param("fader", "gain", 0.0);
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
    fn the_channel_meter_reads_the_channel_rather_than_the_crossfader() {
        let mut strip = ChannelStrip::new(48000.0);
        strip.set_xfader_assign(session_core::XfaderAssign::A);
        strip.set_xfader_position(1.0);

        let mut out = 0.0;
        for _ in 0..48000 {
            out = strip.process_main(1.0, 1.0).0;
        }

        assert!(out.abs() < 0.01, "the deck is crossfaded away, got {out}");
        assert!(strip.metered().0 > 0.99, "meter read {}", strip.metered().0);
    }

    #[test]
    fn the_edit_deck_is_not_a_live_deck() {
        assert!(!crate::audio::LIVE_DECK_IDS.contains(&crate::audio::EDIT_DECK_ID));
    }

    #[test]
    fn the_convenience_constructor_builds_the_manifest_the_app_runs() {
        let strip = ChannelStrip::new(48000.0);
        assert_eq!(strip.manifest.id, crate::audio::MIXER.id);
    }

    fn settled_xfader_gain(strip: &mut ChannelStrip) -> f32 {
        let mut left = 0.0;
        for _ in 0..48000 {
            left = strip.process_main(1.0, 1.0).0;
        }
        left
    }

    #[test]
    fn a_settled_fader_gain_does_not_remember_where_it_came_from() {
        let settled_from = |start: f32| -> f32 {
            let mut strip = ChannelStrip::from_manifest(&session_core::CLASSIC_3BAND_V2, 48000.0);
            strip.set_param(
                session_core::FADER_GAIN.0,
                session_core::FADER_GAIN.1,
                start,
            );
            for _ in 0..96000 {
                strip.process_main(1.0, 1.0);
            }
            strip.set_param(session_core::FADER_GAIN.0, session_core::FADER_GAIN.1, 1.0);
            for _ in 0..96000 {
                strip.process_main(1.0, 1.0);
            }
            strip.process_main(1.0, 1.0).0
        };

        assert_eq!(settled_from(0.0), settled_from(1.0));
    }

    #[test]
    fn a_settled_crossfader_gain_does_not_remember_where_it_came_from() {
        let settled_from = |start: f32| -> f32 {
            let mut strip = ChannelStrip::new(48000.0);
            strip.set_xfader_position(start);
            settled_xfader_gain(&mut strip);
            strip.set_xfader_position(0.0);
            settled_xfader_gain(&mut strip);
            strip.xfader_gain
        };

        assert_eq!(settled_from(-1.0), settled_from(1.0));
        assert_eq!(settled_from(-1.0), settled_from(0.25));
    }

    #[test]
    fn reset_returns_the_crossfader_to_thru_at_centre() {
        let mut strip = ChannelStrip::new(48000.0);
        strip.set_xfader_assign(session_core::XfaderAssign::A);
        strip.set_xfader_position(1.0);
        assert!(settled_xfader_gain(&mut strip) < 0.01);

        strip.reset();

        assert_eq!(strip.xfader_assign, session_core::XfaderAssign::Thru);
        assert!(settled_xfader_gain(&mut strip) > 0.99);
    }

    #[test]
    fn a_crossfader_move_while_stopped_still_converges() {
        let mut strip = ChannelStrip::new(48000.0);
        strip.set_xfader_assign(session_core::XfaderAssign::A);
        strip.consume_settle(usize::MAX);
        assert!(strip.settled(), "the strip should have settled");

        strip.set_xfader_position(1.0);
        assert!(!strip.settled(), "the throw must reopen the window");
        assert!(settled_xfader_gain(&mut strip) < 0.01);
    }

    #[test]
    fn a_thru_strip_is_untouched_wherever_the_crossfader_sits() {
        let mut strip = ChannelStrip::new(48000.0);
        strip.set_xfader_position(-1.0);
        assert!(settled_xfader_gain(&mut strip) > 0.99);
        strip.set_xfader_position(1.0);
        assert!(settled_xfader_gain(&mut strip) > 0.99);
    }

    #[test]
    fn an_assigned_strip_is_cut_at_the_far_end_and_open_at_its_own() {
        let mut strip = ChannelStrip::new(48000.0);
        strip.set_xfader_assign(session_core::XfaderAssign::A);

        strip.set_xfader_position(-1.0);
        assert!(settled_xfader_gain(&mut strip) > 0.99);
        strip.set_xfader_position(1.0);
        assert!(settled_xfader_gain(&mut strip) < 0.001);
    }

    #[test]
    fn assigning_after_the_crossfader_moved_still_resolves() {
        let mut strip = ChannelStrip::new(48000.0);
        strip.set_xfader_position(1.0);
        strip.set_xfader_assign(session_core::XfaderAssign::A);
        assert!(settled_xfader_gain(&mut strip) < 0.001);
    }

    #[test]
    fn the_crossfader_does_not_step_when_thrown() {
        let mut strip = ChannelStrip::new(48000.0);
        strip.set_xfader_assign(session_core::XfaderAssign::A);
        let settled = settled_xfader_gain(&mut strip);
        assert!((settled - 0.707).abs() < 0.01, "centred gain was {settled}");

        strip.set_xfader_position(1.0);
        let first = strip.process_main(1.0, 1.0).0;
        assert!(
            (settled - first).abs() < 0.01,
            "the cut stepped from {settled} to {first} in one sample"
        );
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
        strip.set_param("fader", "gain", 1.0);
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

// State is encoded in three fields: total_frames (0 = empty), is_playing, is_cueing.
#[cfg(test)]
mod cue_state_machine {
    use super::*;

    const SR: u32 = 44100;
    const BPM: f64 = 120.0;

    fn beat_frames() -> f64 {
        (60.0 / BPM) * SR as f64
    }

    fn stopped(duration_secs: f64) -> Deck {
        let mut d = Deck::loaded_for_testing(SR, duration_secs);
        d.is_playing = false;
        d.is_cueing = false;
        d.cue_point = 0.0;
        d
    }

    fn playing(duration_secs: f64) -> Deck {
        let mut d = Deck::loaded_for_testing(SR, duration_secs);
        d.is_playing = true;
        d.is_cueing = false;
        d.cue_point = 0.0;
        d
    }

    fn cueing(duration_secs: f64) -> Deck {
        let mut d = Deck::loaded_for_testing(SR, duration_secs);
        d.is_playing = true;
        d.is_cueing = true;
        d.cue_point = 0.0;
        d
    }

    fn play_command(deck: &mut Deck) {
        crate::audio::apply_deck_command(
            &session_core::SessionCommand::Play {
                deck: "A",
                sec: None,
            },
            deck,
            &mut ChannelStrip::new(SR as f32),
            SR,
            0.0,
            &mut |path: &str| Err(format!("no load: {path}")),
        )
        .expect("play applies");
    }

    #[test]
    fn play_on_a_stopped_deck_starts_it() {
        let mut d = stopped(10.0);
        play_command(&mut d);
        assert!(d.is_playing);
        assert!(!d.is_cueing);
    }

    #[test]
    fn play_during_a_cue_preview_latches_to_playing() {
        let mut d = cueing(10.0);
        d.main_pos = beat_frames() * 2.0 + 1000.0;
        play_command(&mut d);
        assert!(d.is_playing);
        assert!(!d.is_cueing, "cueing flag must be cleared");
        assert!(
            d.main_pos > beat_frames() * 2.0,
            "position stays where playback reached, not snapped back to cue"
        );
    }

    #[test]
    fn press_cue_on_empty_deck_does_nothing() {
        let mut d = Deck::empty(SR);
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
        assert!((d.main_pos - d.cue_point).abs() < 1.0);
    }

    #[test]
    fn press_cue_stopped_within_threshold_of_cue_starts_preview() {
        let mut d = stopped(10.0);
        d.cue_point = beat_frames() * 2.0;
        d.main_pos = d.cue_point + Deck::CUE_THRESHOLD_FRAMES * 0.5; // inside tolerance
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
        let mut d = Deck::empty(SR);
        d.release_cue(); // must not panic
        assert!(!d.is_playing);
    }

    #[test]
    fn cue_point_moved_then_preview_returns_to_new_cue() {
        let mut d = stopped(10.0);
        d.cue_point = 0.0;

        d.main_pos = beat_frames() * 3.0;
        d.press_cue();
        assert_eq!(d.cue_point, beat_frames() * 3.0);

        d.press_cue();
        assert!(d.is_cueing);
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

    fn deck_with_grid(duration_secs: f64) -> Deck {
        let mut d = Deck::loaded_for_testing(SR, duration_secs);
        d.bpm = Some(BPM);
        d.beat_offset_frames = 0.0;
        d
    }

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

    fn deck_and_strip(duration_secs: f64) -> (Deck, ChannelStrip) {
        let mut deck = Deck::loaded_for_testing(SR, duration_secs);
        deck.is_playing = true;
        let mut strip = ChannelStrip::new(SR as f32);
        strip.set_param("eq", "low", 4.0);
        strip.set_param("eq", "high", -6.0);
        strip.set_param("filter", "value", -0.4);
        strip.set_param("filter", "active", 1.0);
        strip.set_param("fader", "gain", 0.3);
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

    fn stopped_pair() -> (Deck, ChannelStrip) {
        let mut deck = Deck::loaded_for_testing(SR, 1.0);
        deck.is_playing = false;
        (deck, ChannelStrip::new(SR as f32))
    }

    fn render(deck: &mut Deck, strip: &mut ChannelStrip, blocks: usize) {
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

        strip.set_param("fader", "gain", 0.25);
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

        strip.set_param("filter", "value", -0.8);
        strip.set_param("filter", "active", 1.0);
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

#[cfg(test)]
mod load_id_tests {
    use super::*;

    const SR: u32 = 44100;

    fn bands_of(frames: usize) -> SpectralBands {
        SpectralBands {
            bass: Arc::new(vec![0.5; frames]),
            ..SpectralBands::default()
        }
    }

    #[test]
    fn points_and_bands_from_a_superseded_load_are_dropped() {
        let mut deck = Deck::empty(SR);
        deck.begin_load(1);
        deck.reset_dense_points(1, 1);
        deck.push_dense_points(1, &[0.1, 0.2, 0.3, 0.4]);

        deck.begin_load(2);
        deck.push_dense_points(1, &[0.9, 0.9, 0.9, 0.9]);
        deck.reset_dense_points(1, 500);
        deck.set_bands(1, bands_of(8));

        assert_eq!(deck.dense_points, [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(deck.bands.frames(), 0);

        deck.reset_dense_points(2, 1);
        deck.push_dense_points(2, &[0.5, 0.5, 0.5, 0.5]);
        deck.set_bands(2, bands_of(8));
        assert_eq!(deck.dense_points, [0.5, 0.5, 0.5, 0.5]);
        assert_eq!(deck.bands.frames(), 8);
    }

    #[test]
    fn a_deck_takes_no_more_points_once_its_load_ends() {
        let mut deck = Deck::empty(SR);
        deck.begin_load(1);
        deck.reset_dense_points(1, 1);

        deck.end_load();
        deck.push_dense_points(1, &[0.9, 0.9, 0.9, 0.9]);

        assert!(deck.dense_points.is_empty());
    }

    #[test]
    fn a_load_landing_late_leaves_a_newer_reduction_running() {
        let mut deck = Deck::empty(SR);
        deck.begin_load(1);
        deck.begin_load(2);

        deck.load("/first.mp3", Arc::new(vec![0.0; 8]), 2, SR);

        deck.reset_dense_points(2, 1);
        deck.push_dense_points(2, &[0.5, 0.5, 0.5, 0.5]);
        assert_eq!(deck.dense_points, [0.5, 0.5, 0.5, 0.5]);
    }
}
