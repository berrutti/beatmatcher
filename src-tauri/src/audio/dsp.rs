// Audio EQ Cookbook biquad (Robert Bristow-Johnson), Direct Form II Transposed.

/// A tail decaying into denormals keeps a bit pattern that depends on the history that got
/// it there, so two mixers on the same signal stop agreeing 800 dB below anything audible.
#[inline]
fn flush_denormal(value: f32) -> f32 {
    if value.abs() < 1.0e-30 {
        0.0
    } else {
        value
    }
}

#[derive(Copy, Clone)]
pub(crate) struct Biquad {
    pub(crate) b0: f32,
    pub(crate) b1: f32,
    pub(crate) b2: f32,
    pub(crate) a1: f32,
    pub(crate) a2: f32,
    pub(crate) delay1: f32,
    pub(crate) delay2: f32,
}

impl Biquad {
    pub(crate) fn identity() -> Self {
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
    pub(crate) fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.delay1;
        self.delay1 = flush_denormal(self.b1 * x - self.a1 * y + self.delay2);
        self.delay2 = flush_denormal(self.b2 * x - self.a2 * y);
        y
    }

    // Carries the delay lines over, or a live signal clicks. Not for the dead zone:
    // identity delay lines are zeroed, never carried over from an active filter.
    #[inline]
    pub(crate) fn set_coefficients(&mut self, src: Self) {
        let d1 = self.delay1;
        let d2 = self.delay2;
        *self = src;
        self.delay1 = d1;
        self.delay2 = d2;
    }

    /// Solved in f64 and narrowed once. An optimiser may reorder equivalent f32 maths into a
    /// form that rounds differently, and a resonant cascade grows that last bit over a sweep.
    fn normalized(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self {
            b0: (b0 / a0) as f32,
            b1: (b1 / a0) as f32,
            b2: (b2 / a0) as f32,
            a1: (a1 / a0) as f32,
            a2: (a2 / a0) as f32,
            delay1: 0.0,
            delay2: 0.0,
        }
    }

    pub(crate) fn low_shelf(sr: f32, freq: f32, db: f32) -> Self {
        if db == 0.0 {
            return Self::identity();
        }
        let a = 10.0f64.powf(f64::from(db) / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * f64::from(freq) / f64::from(sr);
        let cos_w = w0.cos();
        // S = 1 (unity shelf slope) → alpha = sin(w0) / sqrt(2)
        let alpha = w0.sin() / 2.0_f64.sqrt();
        let k = 2.0 * a.sqrt() * alpha;

        Self::normalized(
            a * ((a + 1.0) - (a - 1.0) * cos_w + k),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w),
            a * ((a + 1.0) - (a - 1.0) * cos_w - k),
            (a + 1.0) + (a - 1.0) * cos_w + k,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos_w),
            (a + 1.0) + (a - 1.0) * cos_w - k,
        )
    }

    pub(crate) fn peaking(sr: f32, freq: f32, q: f32, db: f32) -> Self {
        if db == 0.0 {
            return Self::identity();
        }
        let a = 10.0f64.powf(f64::from(db) / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * f64::from(freq) / f64::from(sr);
        let alpha = w0.sin() / (2.0 * f64::from(q));
        let cos_w = w0.cos();

        Self::normalized(
            1.0 + alpha * a,
            -2.0 * cos_w,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cos_w,
            1.0 - alpha / a,
        )
    }

    pub(crate) fn low_pass(sr: f32, freq: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * f64::from(freq) / f64::from(sr);
        let cos_w = w0.cos();
        let alpha = w0.sin() / (2.0 * f64::from(q));

        Self::normalized(
            (1.0 - cos_w) / 2.0,
            1.0 - cos_w,
            (1.0 - cos_w) / 2.0,
            1.0 + alpha,
            -2.0 * cos_w,
            1.0 - alpha,
        )
    }

    pub(crate) fn high_pass(sr: f32, freq: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * f64::from(freq) / f64::from(sr);
        let cos_w = w0.cos();
        let alpha = w0.sin() / (2.0 * f64::from(q));

        Self::normalized(
            (1.0 + cos_w) / 2.0,
            -(1.0 + cos_w),
            (1.0 + cos_w) / 2.0,
            1.0 + alpha,
            -2.0 * cos_w,
            1.0 - alpha,
        )
    }

    pub(crate) fn high_shelf(sr: f32, freq: f32, db: f32) -> Self {
        if db == 0.0 {
            return Self::identity();
        }
        let a = 10.0f64.powf(f64::from(db) / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * f64::from(freq) / f64::from(sr);
        let cos_w = w0.cos();
        let alpha = w0.sin() / 2.0_f64.sqrt();
        let k = 2.0 * a.sqrt() * alpha;

        Self::normalized(
            a * ((a + 1.0) + (a - 1.0) * cos_w + k),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w),
            a * ((a + 1.0) + (a - 1.0) * cos_w - k),
            (a + 1.0) - (a - 1.0) * cos_w + k,
            2.0 * ((a - 1.0) - (a + 1.0) * cos_w),
            (a + 1.0) - (a - 1.0) * cos_w - k,
        )
    }
}

pub(crate) struct Equalizer {
    sample_rate: f32,
    voicing: &'static session_core::Eq3Voicing,
    low_stage1: [Biquad; 2],
    low_stage2: [Biquad; 2],
    mid: [Biquad; 2],
    high_stage1: [Biquad; 2],
    high_stage2: [Biquad; 2],
}

impl Equalizer {
    pub(crate) fn new(sample_rate: f32, voicing: &'static session_core::Eq3Voicing) -> Self {
        Self {
            sample_rate,
            voicing,
            low_stage1: [Biquad::identity(), Biquad::identity()],
            low_stage2: [Biquad::identity(), Biquad::identity()],
            mid: [Biquad::identity(), Biquad::identity()],
            high_stage1: [Biquad::identity(), Biquad::identity()],
            high_stage2: [Biquad::identity(), Biquad::identity()],
        }
    }

    pub(crate) fn set_low(&mut self, db: f32) {
        let stage = Biquad::low_shelf(self.sample_rate, self.voicing.low_shelf_hz, db / 2.0);
        for (first, second) in self.low_stage1.iter_mut().zip(self.low_stage2.iter_mut()) {
            first.set_coefficients(stage);
            second.set_coefficients(stage);
        }
    }

    pub(crate) fn set_mid(&mut self, db: f32) {
        let filter = Biquad::peaking(
            self.sample_rate,
            self.voicing.mid_peak_hz,
            self.voicing.mid_q,
            db,
        );
        for ch in &mut self.mid {
            ch.set_coefficients(filter);
        }
    }

    pub(crate) fn set_high(&mut self, db: f32) {
        let stage = Biquad::high_shelf(self.sample_rate, self.voicing.high_shelf_hz, db / 2.0);
        for (first, second) in self.high_stage1.iter_mut().zip(self.high_stage2.iter_mut()) {
            first.set_coefficients(stage);
            second.set_coefficients(stage);
        }
    }

    #[inline]
    pub(crate) fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let l = self.low_stage2[0].process(self.low_stage1[0].process(l));
        let r = self.low_stage2[1].process(self.low_stage1[1].process(r));
        let l = self.mid[0].process(l);
        let r = self.mid[1].process(r);
        let l = self.high_stage2[0].process(self.high_stage1[0].process(l));
        let r = self.high_stage2[1].process(self.high_stage1[1].process(r));
        (l, r)
    }
}

fn eq4_mid_q(table: &[(f32, f32)], db: f32) -> f32 {
    let above = table.partition_point(|&(entry_db, _)| entry_db < db);
    let below = above.checked_sub(1).and_then(|index| table.get(index));
    match (below, table.get(above)) {
        (Some(&(low_db, low_q)), Some(&(high_db, high_q))) => {
            low_q + (high_q - low_q) * (db - low_db) / (high_db - low_db)
        }
        (Some(&(_, q)), None) | (None, Some(&(_, q))) => q,
        // Unreachable on a validated manifest. Butterworth keeps the bell finite rather
        // than panicking on the audio thread.
        (None, None) => std::f32::consts::FRAC_1_SQRT_2,
    }
}

/// The crossfade below the detent is what the analogue stage does: it sweeps its filter's
/// shunt reference between the stage input and ground.
struct ShelfBand {
    shelf: [Biquad; 2],
    kill: [Biquad; 2],
    kill_mix: f32,
    range: EqRange,
}

impl ShelfBand {
    fn new(kill: Biquad, range: EqRange) -> Self {
        Self {
            shelf: [Biquad::identity(), Biquad::identity()],
            kill: [kill, kill],
            kill_mix: 0.0,
            range,
        }
    }

    fn set(&mut self, db: f32, shelf: impl Fn(f32) -> Biquad) {
        let db = db.clamp(self.range.min_db, self.range.max_db);
        if db >= 0.0 {
            let stage = shelf(db);
            for channel in &mut self.shelf {
                channel.set_coefficients(stage);
            }
            self.kill_mix = 0.0;
        } else {
            let flat = Biquad::identity();
            for channel in &mut self.shelf {
                channel.set_coefficients(flat);
            }
            self.kill_mix = db / self.range.min_db;
        }
    }

    // Fixed coefficients, so it keeps running while mixed out rather than restarting cold.
    #[inline]
    fn process(&mut self, channel: usize, x: f32) -> f32 {
        let shelved = self.shelf[channel].process(x);
        let killed = self.kill[channel].process(shelved);
        shelved + (killed - shelved) * self.kill_mix
    }
}

#[derive(Clone, Copy)]
pub(crate) struct EqRange {
    pub(crate) min_db: f32,
    pub(crate) max_db: f32,
}

pub(crate) struct Equalizer4 {
    sample_rate: f32,
    voicing: &'static session_core::Eq4Voicing,
    low: ShelfBand,
    low_mid: [Biquad; 2],
    high_mid: [Biquad; 2],
    high: ShelfBand,
}

impl Equalizer4 {
    pub(crate) fn new(
        sample_rate: f32,
        voicing: &'static session_core::Eq4Voicing,
        low_range: EqRange,
        high_range: EqRange,
    ) -> Self {
        let low_kill = Biquad::high_pass(sample_rate, voicing.low_hz, voicing.low_kill_q);
        let high_kill = Biquad::low_pass(sample_rate, voicing.high_hz, voicing.high_kill_q);
        Self {
            sample_rate,
            voicing,
            low: ShelfBand::new(low_kill, low_range),
            low_mid: [Biquad::identity(), Biquad::identity()],
            high_mid: [Biquad::identity(), Biquad::identity()],
            high: ShelfBand::new(high_kill, high_range),
        }
    }

    pub(crate) fn set_low(&mut self, db: f32) {
        let (sample_rate, hz) = (self.sample_rate, self.voicing.low_hz);
        self.low
            .set(db, |db| Biquad::low_shelf(sample_rate, hz, db));
    }

    pub(crate) fn set_high(&mut self, db: f32) {
        let (sample_rate, hz) = (self.sample_rate, self.voicing.high_hz);
        self.high
            .set(db, |db| Biquad::high_shelf(sample_rate, hz, db));
    }

    pub(crate) fn set_low_mid(&mut self, db: f32) {
        let filter = Biquad::peaking(
            self.sample_rate,
            self.voicing.low_mid_hz,
            eq4_mid_q(self.voicing.mid_q_by_db, db),
            db,
        );
        for channel in &mut self.low_mid {
            channel.set_coefficients(filter);
        }
    }

    pub(crate) fn set_high_mid(&mut self, db: f32) {
        let filter = Biquad::peaking(
            self.sample_rate,
            self.voicing.high_mid_hz,
            eq4_mid_q(self.voicing.mid_q_by_db, db),
            db,
        );
        for channel in &mut self.high_mid {
            channel.set_coefficients(filter);
        }
    }

    #[inline]
    pub(crate) fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let l = self.low.process(0, l);
        let r = self.low.process(1, r);
        let l = self.low_mid[0].process(l);
        let r = self.low_mid[1].process(r);
        let l = self.high_mid[0].process(l);
        let r = self.high_mid[1].process(r);
        let l = self.high.process(0, l);
        let r = self.high.process(1, r);
        (l, r)
    }
}

// Negative sweeps the lowpass down from 20 kHz, positive the highpass up from 20 Hz.

const FILTER_MIN_FREQ_HZ: f32 = 20.0;
const FILTER_MAX_FREQ_HZ: f32 = 20_000.0;
pub(crate) const FILTER_RESONANCE_Q: f32 = 2.0;
// Q used at sweep=0 (center/dead-zone boundary). Interpolated up to
// FILTER_RESONANCE_Q as the knob sweeps toward the extremes. Butterworth (0.5)
// gives a smooth, flat response at the entry point of the sweep with no
// resonance bump.
const FILTER_CENTER_Q: f32 = 0.5;
const FILTER_CENTER_DEAD_ZONE: f32 = session_core::FILTER_DEAD_ZONE as f32;
// The filtered path is faded in over this much knob travel above the dead zone,
// so the identity reset at the boundary lands while nothing can hear it.
const FILTER_ENTRY_WIDTH: f32 = 0.05;
const FILTER_SMOOTHING_TAU_SEC: f32 = 0.015;
// The filter output is crossfaded with the dry signal so the knob position never
// sweeps during a bypass transition.
pub(crate) const FILTER_CROSSFADE_TAU_SEC: f32 = 0.05;
// Small enough to avoid click artifacts at high Q, large enough to keep CPU light.
pub(crate) const FILTER_COEFF_REFRESH_INTERVAL: u32 = 4;
// Beyond this sweep fraction (0..1) the output gain fades linearly to 0 so the
// extreme position reaches -infinity regardless of biquad rolloff slope.
const FILTER_KILL_START: f32 = 0.80;

// |H|max of one resonant section, solved off the analog prototype
// `1/(s^2 + s/q + 1)`. Two in series reach its square.
fn resonant_peak(q: f32) -> f32 {
    // Below Butterworth the response is monotonic, so the peak is the passband.
    if q <= std::f32::consts::FRAC_1_SQRT_2 {
        return 1.0;
    }
    q / (1.0 - 1.0 / (4.0 * q * q)).sqrt()
}

pub(crate) struct Filter {
    sample_rate: f32,
    target_knob: f32,
    current_knob: f32,
    target_resonance: f32,
    current_resonance: f32,
    smoothing_coeff: f32,
    coeff_refresh_counter: u32,
    // Two cascaded 2nd-order stages per channel give 4th-order (-24 dB/oct) rolloff.
    filters_a: [Biquad; 2],
    filters_b: [Biquad; 2],
    makeup: f32,
    // Dry/wet crossfade: 0.0 = fully filtered, 1.0 = fully dry (bypassed).
    // The filter always runs at its set position. Bypass is a gain crossfade,
    // never a frequency sweep.
    crossfade: f32,
    crossfade_target: f32,
    crossfade_coeff: f32,
}

impl Filter {
    pub(crate) fn new(sample_rate: f32) -> Self {
        let smoothing_coeff = 1.0 - (-1.0 / (sample_rate * FILTER_SMOOTHING_TAU_SEC)).exp();
        let crossfade_coeff = 1.0 - (-1.0 / (sample_rate * FILTER_CROSSFADE_TAU_SEC)).exp();
        Self {
            sample_rate,
            target_knob: 0.0,
            current_knob: 0.0,
            target_resonance: FILTER_RESONANCE_Q,
            current_resonance: FILTER_RESONANCE_Q,
            smoothing_coeff,
            coeff_refresh_counter: 0,
            filters_a: [Biquad::identity(), Biquad::identity()],
            filters_b: [Biquad::identity(), Biquad::identity()],
            makeup: 1.0,
            crossfade: 1.0,
            crossfade_target: 1.0,
            crossfade_coeff,
        }
    }

    pub(crate) fn set_knob(&mut self, v: f32) {
        self.target_knob = v.clamp(-1.0, 1.0);
        self.coeff_refresh_counter = 0;
    }

    pub(crate) fn set_resonance(&mut self, q: f32) {
        self.target_resonance = q.max(FILTER_CENTER_Q);
        self.coeff_refresh_counter = 0;
    }

    pub(crate) fn set_active(&mut self, active: bool) {
        self.crossfade_target = if active { 0.0 } else { 1.0 };
    }

    fn update_filters(&mut self) {
        let knob = self.current_knob;
        let abs_knob = knob.abs();

        if abs_knob <= FILTER_CENTER_DEAD_ZONE {
            // Preserving delay lines here would allow IIR state from the previous
            // active filter to ring through, causing transient overshoots that push
            // samples above 1.0.
            let identity = Biquad::identity();
            for (a, b) in self.filters_a.iter_mut().zip(self.filters_b.iter_mut()) {
                *a = identity;
                *b = identity;
            }
            self.makeup = 1.0;
            return;
        }

        let sweep = (abs_knob - FILTER_CENTER_DEAD_ZONE) / (1.0 - FILTER_CENTER_DEAD_ZONE);
        let q = FILTER_CENTER_Q + (self.current_resonance - FILTER_CENTER_Q) * sweep;
        let peak = resonant_peak(q);
        self.makeup = 1.0 / (peak * peak);
        let new_filter = if knob < 0.0 {
            let cutoff = FILTER_MAX_FREQ_HZ * (FILTER_MIN_FREQ_HZ / FILTER_MAX_FREQ_HZ).powf(sweep);
            Biquad::low_pass(self.sample_rate, cutoff, q)
        } else {
            let cutoff = FILTER_MIN_FREQ_HZ * (FILTER_MAX_FREQ_HZ / FILTER_MIN_FREQ_HZ).powf(sweep);
            Biquad::high_pass(self.sample_rate, cutoff, q)
        };

        for (a, b) in self.filters_a.iter_mut().zip(self.filters_b.iter_mut()) {
            a.set_coefficients(new_filter);
            b.set_coefficients(new_filter);
        }
    }

    #[inline]
    pub(crate) fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.current_knob =
            super::unit::approach(self.current_knob, self.target_knob, self.smoothing_coeff);
        self.current_resonance = super::unit::approach(
            self.current_resonance,
            self.target_resonance,
            self.smoothing_coeff,
        );

        if self.coeff_refresh_counter == 0 {
            self.update_filters();
        }
        self.coeff_refresh_counter =
            (self.coeff_refresh_counter + 1) % FILTER_COEFF_REFRESH_INTERVAL;

        let abs_knob = self.current_knob.abs();
        let kill_gain = if abs_knob > FILTER_CENTER_DEAD_ZONE {
            let sweep = (abs_knob - FILTER_CENTER_DEAD_ZONE) / (1.0 - FILTER_CENTER_DEAD_ZONE);
            if sweep > FILTER_KILL_START {
                1.0 - (sweep - FILTER_KILL_START) / (1.0 - FILTER_KILL_START)
            } else {
                1.0
            }
        } else {
            1.0
        };

        let gain = kill_gain * self.makeup;
        let l_filtered = self.filters_b[0].process(self.filters_a[0].process(l)) * gain;
        let r_filtered = self.filters_b[1].process(self.filters_a[1].process(r)) * gain;

        self.crossfade =
            super::unit::approach(self.crossfade, self.crossfade_target, self.crossfade_coeff);
        let entry = ((abs_knob - FILTER_CENTER_DEAD_ZONE) / FILTER_ENTRY_WIDTH).clamp(0.0, 1.0);
        let wet = (1.0 - self.crossfade) * entry;
        (
            l * (1.0 - wet) + l_filtered * wet,
            r * (1.0 - wet) + r_filtered * wet,
        )
    }
}

// Freeverb's tunings, by Jezar at Dreampoint, public domain.
const REVERB_TUNING_SR: f32 = 44_100.0;
const REVERB_COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const REVERB_ALLPASSES: [usize; 4] = [556, 441, 341, 225];
// The right channel's lines run this much longer, which is what makes the tail wide.
const REVERB_STEREO_SPREAD: usize = 23;
const REVERB_ALLPASS_FEEDBACK: f32 = 0.5;
const REVERB_DAMPING: f32 = 0.2;
// Freeverb's allpasses are not true allpasses: each raises the average power 7/3 times, so
// the four come back about 14.7 dB above what entered them.
const REVERB_OUTPUT_GAIN: f32 = 9.0 / 49.0;

const REVERB_SMOOTHING_TAU_SEC: f32 = 0.020;

struct Comb {
    line: Vec<f32>,
    index: usize,
    damped: f32,
    feedback: f32,
    feedback_target: f32,
}

impl Comb {
    fn new(length: usize) -> Self {
        Self {
            line: vec![0.0; length],
            index: 0,
            damped: 0.0,
            feedback: 0.0,
            feedback_target: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, input: f32, smoothing_coeff: f32) -> f32 {
        self.feedback = super::unit::approach(self.feedback, self.feedback_target, smoothing_coeff);
        let level = (1.0 - self.feedback * self.feedback).sqrt();
        let output = self.line[self.index];
        self.damped =
            flush_denormal(output * (1.0 - REVERB_DAMPING) + self.damped * REVERB_DAMPING);
        self.line[self.index] = flush_denormal(input * level + self.damped * self.feedback);
        self.index = (self.index + 1) % self.line.len();
        output
    }
}

struct Allpass {
    line: Vec<f32>,
    index: usize,
}

impl Allpass {
    fn new(length: usize) -> Self {
        Self {
            line: vec![0.0; length],
            index: 0,
        }
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let delayed = self.line[self.index];
        self.line[self.index] = flush_denormal(input + delayed * REVERB_ALLPASS_FEEDBACK);
        self.index = (self.index + 1) % self.line.len();
        delayed - input
    }
}

pub(crate) struct Reverb {
    sample_rate: f32,
    tail_ms: f32,
    combs: [[Comb; REVERB_COMBS.len()]; 2],
    allpasses: [[Allpass; REVERB_ALLPASSES.len()]; 2],
    smoothing_coeff: f32,
}

impl Reverb {
    pub(crate) fn new(sample_rate: f32) -> Self {
        let length = |tuning: usize, channel: usize| {
            let samples = (tuning + channel * REVERB_STEREO_SPREAD) as f32;
            ((samples * sample_rate / REVERB_TUNING_SR).round() as usize).max(1)
        };
        Self {
            sample_rate,
            tail_ms: 0.0,
            combs: std::array::from_fn(|channel| {
                std::array::from_fn(|index| Comb::new(length(REVERB_COMBS[index], channel)))
            }),
            allpasses: std::array::from_fn(|channel| {
                std::array::from_fn(|index| Allpass::new(length(REVERB_ALLPASSES[index], channel)))
            }),
            smoothing_coeff: 1.0 - (-1.0 / (sample_rate * REVERB_SMOOTHING_TAU_SEC)).exp(),
        }
    }

    pub(crate) fn settle(&mut self) {
        for comb in self.combs.iter_mut().flatten() {
            comb.feedback = comb.feedback_target;
        }
    }

    pub(crate) fn set_tail_ms(&mut self, ms: f32) {
        self.tail_ms = ms.max(1.0);
        let tail_frames = self.tail_ms * self.sample_rate / 1000.0;
        for comb in self.combs.iter_mut().flatten() {
            comb.feedback_target = 10f32.powf(-3.0 * comb.line.len() as f32 / tail_frames);
        }
    }

    pub(crate) fn ring_out_frames(&self) -> usize {
        let longest = self
            .combs
            .iter()
            .flatten()
            .map(|comb| comb.line.len())
            .max()
            .unwrap_or(0);
        (self.tail_ms * self.sample_rate / 1000.0) as usize + longest
    }

    #[inline]
    pub(crate) fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let input = (l + r) * 0.5 / (REVERB_COMBS.len() as f32).sqrt();
        let smoothing_coeff = self.smoothing_coeff;
        let mut out = [0.0f32; 2];
        for (channel, sum) in out.iter_mut().enumerate() {
            *sum = self.combs[channel]
                .iter_mut()
                .map(|comb| comb.process(input, smoothing_coeff))
                .sum();
            for allpass in &mut self.allpasses[channel] {
                *sum = allpass.process(*sum);
            }
        }
        (out[0] * REVERB_OUTPUT_GAIN, out[1] * REVERB_OUTPUT_GAIN)
    }

    pub(crate) fn mix_block(&mut self, send: &[f32], mix: &mut [f32]) {
        for (frame, pair) in send.chunks_exact(2).enumerate() {
            let (l, r) = self.process(pair[0], pair[1]);
            mix[frame * 2] += l;
            mix[frame * 2 + 1] += r;
        }
    }
}

/// The master stage over a whole block: gain, then the limiter, in place. Both the
/// callback and the offline render run this same pass over their mix buffer.
pub(crate) fn master_block(limiter: Option<&mut Limiter>, gain: f32, mix: &mut [f32]) {
    match limiter {
        Some(limiter) => {
            for frame in mix.chunks_exact_mut(2) {
                let (l, r) = limiter.process(frame[0] * gain, frame[1] * gain);
                frame[0] = l;
                frame[1] = r;
            }
        }
        None => {
            for sample in mix.iter_mut() {
                *sample = (*sample * gain).clamp(-1.0, 1.0);
            }
        }
    }
}

/// Every output path ends here, so the live and offline chains cannot drift apart.
#[inline]
pub(crate) fn master_output(limiter: Option<&mut Limiter>, l: f32, r: f32) -> (f32, f32) {
    match limiter {
        Some(limiter) => limiter.process(l, r),
        None => (l.clamp(-1.0, 1.0), r.clamp(-1.0, 1.0)),
    }
}

// True-peak brickwall: instantaneous attack (no sample exceeds THRESHOLD), ~150ms release.

pub(crate) struct Limiter {
    pub(crate) gain_reduction: f32,
    release_coeff: f32,
}

impl Limiter {
    pub(crate) const THRESHOLD: f32 = 0.99;
    const RELEASE_TAU_SEC: f32 = 0.150;

    pub(crate) fn new(sample_rate: f32) -> Self {
        Self {
            gain_reduction: 1.0,
            release_coeff: 1.0 - (-1.0 / (sample_rate * Self::RELEASE_TAU_SEC)).exp(),
        }
    }

    // Clamp is a safety net for floating-point edge cases.
    #[inline]
    pub(crate) fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let peak = l.abs().max(r.abs());
        let target_gr = if peak > 0.0 {
            (Self::THRESHOLD / peak).min(1.0)
        } else {
            1.0
        };
        if target_gr < self.gain_reduction {
            self.gain_reduction = target_gr;
        } else {
            self.gain_reduction =
                super::unit::approach(self.gain_reduction, target_gr, self.release_coeff);
        }
        (
            (l * self.gain_reduction).clamp(-1.0, 1.0),
            (r * self.gain_reduction).clamp(-1.0, 1.0),
        )
    }
}

#[cfg(test)]
mod coefficient_refresh_phase {
    use super::*;

    fn swept(pre_roll: u32) -> Vec<f32> {
        let mut filter = Filter::new(44_100.0);
        filter.set_active(true);
        for _ in 0..80_000 {
            filter.process(0.0, 0.0);
        }
        for _ in 0..pre_roll {
            filter.process(0.0, 0.0);
        }
        let mut out = Vec::new();
        for step in 0..8 {
            filter.set_knob(0.05 + 0.03 * step as f32);
            for frame in 0..600 {
                let x = ((frame + step * 600) as f32 * 0.01).sin();
                let (l, _) = filter.process(x, x);
                out.push(l);
            }
        }
        out
    }

    #[test]
    fn a_sweep_does_not_depend_on_how_long_the_filter_has_been_running() {
        let reference = swept(0);
        for pre_roll in 1..8 {
            let other = swept(pre_roll);
            let differing = reference.iter().zip(&other).filter(|(a, b)| a != b).count();
            assert_eq!(
                differing, 0,
                "a pre-roll of {pre_roll} samples changed the sweep"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_biquad_fed_silence_settles_on_exactly_zero() {
        let mut filter = Biquad::peaking(48_000.0, 1_000.0, 0.7, 6.0);
        for _ in 0..64 {
            filter.process(1.0);
        }
        for _ in 0..4_000_000 {
            filter.process(0.0);
        }
        assert_eq!(filter.delay1, 0.0);
        assert_eq!(filter.delay2, 0.0);
    }

    use super::*;

    fn sine_wave(freq_hz: f32, sample_rate: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq_hz * i as f32 / sample_rate).sin())
            .collect()
    }

    #[test]
    fn identity_biquad_passes_impulse_through() {
        let mut bq = Biquad::identity();
        assert_eq!(bq.process(1.0), 1.0);
        for _ in 0..63 {
            assert_eq!(bq.process(0.0), 0.0);
        }
    }

    #[test]
    fn identity_biquad_l1_norm_is_one() {
        let mut bq = Biquad::identity();
        let norm: f32 = std::iter::once(1.0_f32)
            .chain(std::iter::repeat(0.0).take(127))
            .map(|x| bq.process(x).abs())
            .sum();
        assert!((norm - 1.0).abs() < 1e-6, "L1 norm={}", norm);
    }

    #[test]
    fn set_coefficients_preserves_delay_lines() {
        let mut bq = Biquad::identity();
        bq.delay1 = 0.5;
        bq.delay2 = 0.25;
        let lpf = Biquad::low_pass(44100.0, 1000.0, 0.707);
        bq.set_coefficients(lpf);
        assert_eq!(bq.delay1, 0.5);
        assert_eq!(bq.delay2, 0.25);
        assert_eq!(bq.b0, lpf.b0);
    }

    #[test]
    fn low_pass_attenuates_above_cutoff() {
        let sr = 44100.0f32;
        let mut bq = Biquad::low_pass(sr, 1000.0, 0.707);
        for i in 0..4096 {
            bq.process((2.0 * std::f32::consts::PI * 4000.0 * i as f32 / sr).sin());
        }
        // Measure steady-state RMS at 4x the cutoff. A 2nd-order Butterworth LPF
        // rolls off at -12 dB/octave, so 4 kHz is 2 octaves above the 1 kHz cutoff:
        // expected attenuation ~24 dB, amplitude factor ~1/16.
        let mut sum_sq = 0.0f32;
        for i in 4096..8192 {
            let y = bq.process((2.0 * std::f32::consts::PI * 4000.0 * i as f32 / sr).sin());
            sum_sq += y * y;
        }
        let rms = (sum_sq / 4096.0).sqrt();
        assert!(
            rms < 0.1,
            "LPF@1kHz should strongly attenuate 4kHz, got RMS={}",
            rms
        );
    }

    #[test]
    fn high_pass_attenuates_below_cutoff() {
        let sr = 44100.0f32;
        let mut bq = Biquad::high_pass(sr, 1000.0, 0.707);
        for i in 0..4096 {
            bq.process((2.0 * std::f32::consts::PI * 250.0 * i as f32 / sr).sin());
        }
        let mut sum_sq = 0.0f32;
        for i in 4096..8192 {
            let y = bq.process((2.0 * std::f32::consts::PI * 250.0 * i as f32 / sr).sin());
            sum_sq += y * y;
        }
        let rms = (sum_sq / 4096.0).sqrt();
        assert!(
            rms < 0.1,
            "HPF@1kHz should strongly attenuate 250Hz, got RMS={}",
            rms
        );
    }

    #[test]
    fn filter_center_impulse_never_clips() {
        let mut state = Filter::new(44100.0);
        let (l, r) = state.process(1.0, 1.0);
        assert!(l <= 1.0 + 1e-5 && r <= 1.0 + 1e-5, "l={} r={}", l, r);
        for _ in 0..32 {
            let (l, r) = state.process(0.0, 0.0);
            assert!(l.abs() < 1e-6 && r.abs() < 1e-6, "l={} r={}", l, r);
        }
    }

    #[test]
    fn filter_center_full_scale_sine_never_clips() {
        let sr = 44100.0f32;
        let mut state = Filter::new(sr);
        for (i, &s) in sine_wave(1000.0, sr, 4096).iter().enumerate() {
            let (l, r) = state.process(s, s);
            assert!(l.abs() <= 1.0 + 1e-5, "clipped at sample {}: l={}", i, l);
            assert!(r.abs() <= 1.0 + 1e-5, "clipped at sample {}: r={}", i, r);
        }
    }

    #[test]
    fn filter_center_multiple_frequencies_never_clip() {
        let sr = 44100.0f32;
        for &freq in &[40.0f32, 200.0, 1000.0, 8000.0, 15000.0] {
            let mut state = Filter::new(sr);
            for (i, &s) in sine_wave(freq, sr, 4096).iter().enumerate() {
                let (l, _r) = state.process(s, s);
                assert!(
                    l.abs() <= 1.0 + 1e-5,
                    "freq={} Hz clipped at sample {}: l={}",
                    freq,
                    i,
                    l
                );
            }
        }
    }

    // Leaves room for the f32 arithmetic without admitting anything audible.
    const FULL_SCALE_SLACK: f32 = 1e-3;
    // A sine cannot step, so anything past its own per-sample slew is a click.
    const STEP_OVER_NATURAL_SLEW: f32 = 4.0;
    // Energy stored in a resonator being swept, measured at +0.6 dB worst case.
    const SWEEP_TRANSIENT_CEILING: f32 = 1.1;

    // The frequency each knob position responds loudest at, so the sine below
    // drives the worst case instead of an arbitrary tone.
    fn filter_worst_hz(knob: f32) -> f32 {
        let abs_knob = knob.abs();
        if abs_knob <= FILTER_CENTER_DEAD_ZONE {
            return 1_000.0;
        }
        let sweep = (abs_knob - FILTER_CENTER_DEAD_ZONE) / (1.0 - FILTER_CENTER_DEAD_ZONE);
        let q = FILTER_CENTER_Q + (FILTER_RESONANCE_Q - FILTER_CENTER_Q) * sweep;
        let lowpass = knob < 0.0;
        let cutoff = if lowpass {
            FILTER_MAX_FREQ_HZ * (FILTER_MIN_FREQ_HZ / FILTER_MAX_FREQ_HZ).powf(sweep)
        } else {
            FILTER_MIN_FREQ_HZ * (FILTER_MAX_FREQ_HZ / FILTER_MIN_FREQ_HZ).powf(sweep)
        };
        let shift = 1.0 - 1.0 / (2.0 * q * q);
        if shift <= 0.0 {
            return if lowpass {
                FILTER_MIN_FREQ_HZ
            } else {
                FILTER_MAX_FREQ_HZ
            };
        }
        let hz = if lowpass {
            cutoff * shift.sqrt()
        } else {
            cutoff / shift.sqrt()
        };
        hz.clamp(10.0, FILTER_MAX_FREQ_HZ)
    }

    #[test]
    fn no_knob_position_lets_a_full_scale_sine_out_above_full_scale() {
        let sr = 44_100.0f32;
        let frames = sr as usize / 2;
        for step in 0..=40 {
            let knob = -1.0 + step as f32 / 20.0;
            let hz = filter_worst_hz(knob);
            let mut state = Filter::new(sr);
            state.set_active(true);
            state.set_knob(knob);
            let mut peak = 0.0f32;
            for frame in 0..frames {
                let x = (std::f32::consts::TAU * hz * frame as f32 / sr).sin();
                let (l, r) = state.process(x, x);
                if frame > frames / 2 {
                    peak = peak.max(l.abs().max(r.abs()));
                }
            }
            assert!(
                peak <= 1.0 + FULL_SCALE_SLACK,
                "knob {knob} at {hz} Hz peaked at {peak}"
            );
        }
    }

    #[test]
    fn sweeping_across_the_centre_never_steps_the_output() {
        let sr = 44_100.0f32;
        let hz = 60.0f32;
        let natural_step = (std::f32::consts::TAU * hz / sr).sin();
        let sine = |frame: usize| (std::f32::consts::TAU * hz * frame as f32 / sr).sin();
        let mut state = Filter::new(sr);
        state.set_active(true);
        state.set_knob(0.2);

        let mut frame = 0usize;
        let mut previous = 0.0f32;
        for _ in 0..(sr as usize / 4) {
            previous = state.process(sine(frame), sine(frame)).0;
            frame += 1;
        }

        let sweep_frames = sr as usize / 5;
        for step in 0..sweep_frames {
            state.set_knob(0.2 - 0.4 * (step as f32 / sweep_frames as f32));
            let (l, _r) = state.process(sine(frame), sine(frame));
            frame += 1;
            let jump = (l - previous).abs();
            assert!(
                jump <= natural_step * STEP_OVER_NATURAL_SLEW,
                "output jumped {jump} at sample {step}, the sine itself only moves {natural_step}"
            );
            previous = l;
        }
    }

    #[test]
    fn random_knob_gestures_stay_inside_one_sweep_transient_of_full_scale() {
        let sr = 44_100.0f32;
        let mut seed = 0x2545_F491u32;
        let mut random = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32
        };
        const CASES: usize = 60;
        const LOWEST_HZ: f32 = 25.0;
        const HIGHEST_HZ: f32 = 14_000.0;
        const SHORTEST_GESTURE_SEC: f32 = 0.2;
        const LONGEST_GESTURE_SEC: f32 = 0.6;
        const SETTLING_SEC: f32 = 0.1;

        for case in 0..CASES {
            let from = random() * 2.0 - 1.0;
            let to = random() * 2.0 - 1.0;
            let hz = LOWEST_HZ + random() * (HIGHEST_HZ - LOWEST_HZ);
            let gesture_sec =
                SHORTEST_GESTURE_SEC + random() * (LONGEST_GESTURE_SEC - SHORTEST_GESTURE_SEC);
            let frames = (sr * gesture_sec) as usize;
            let sine = |frame: usize| (std::f32::consts::TAU * hz * frame as f32 / sr).sin();
            let mut state = Filter::new(sr);
            state.set_active(true);
            state.set_knob(from);

            let mut frame = 0usize;
            for _ in 0..((sr * SETTLING_SEC) as usize) {
                state.process(sine(frame), sine(frame));
                frame += 1;
            }
            let mut peak = 0.0f32;
            for step in 0..frames {
                state.set_knob(from + (to - from) * (step as f32 / frames as f32));
                let (l, r) = state.process(sine(frame), sine(frame));
                frame += 1;
                peak = peak.max(l.abs().max(r.abs()));
            }
            assert!(
                peak <= SWEEP_TRANSIENT_CEILING,
                "case {case}: {from} -> {to} at {hz} Hz peaked at {peak}"
            );
        }
    }

    const EQ_MIN_DB: f32 = session_core::EQ_MIN_DB as f32;
    const EQ_MAX_DB: f32 = session_core::EQ_MAX_DB as f32;

    pub(super) fn classic_eq4(sample_rate: f32) -> Equalizer4 {
        let slot = session_core::CLASSIC_4BAND
            .strip_slot("eq")
            .expect("an eq slot");
        let session_core::Voicing::Eq4(voicing) = slot.voicing else {
            panic!("the four band is voiced as one");
        };
        let range = EqRange {
            min_db: EQ_MIN_DB,
            max_db: EQ_MAX_DB,
        };
        Equalizer4::new(sample_rate, voicing, range, range)
    }

    #[test]
    fn eq_at_its_detents_is_transparent() {
        let sr = 44100.0f32;
        let mut eq = classic_eq4(sr);
        eq.set_low(0.0);
        eq.set_high(0.0);
        for &s in sine_wave(1000.0, sr, 1024).iter() {
            let (l, _r) = eq.process(s, s);
            assert!((l - s).abs() < 1e-6, "input={} output={}", s, l);
        }
    }

    fn rms(signal: &[f32]) -> f32 {
        let sum_sq: f32 = signal.iter().map(|&x| x * x).sum();
        (sum_sq / signal.len() as f32).sqrt()
    }

    fn band_rms(hz: f32, set: impl Fn(&mut Equalizer4)) -> f32 {
        let sr = 44100.0f32;
        let input: Vec<f32> = (0..8192)
            .map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / sr).sin())
            .collect();
        let mut eq = classic_eq4(sr);
        set(&mut eq);
        let out: Vec<f32> = input.iter().map(|&s| eq.process(s, s).0).collect();
        rms(&out[4096..])
    }

    #[test]
    fn a_shelf_swept_to_its_end_rolls_its_band_off_at_12_db_per_octave() {
        let at = |hz: f32, set: fn(&mut Equalizer4)| 20.0 * band_rms(hz, set).log10();
        let one_octave = at(1250.0, |eq| eq.set_high(EQ_MIN_DB));
        let two_octaves = at(2500.0 * 2.0, |eq| eq.set_high(EQ_MIN_DB));
        assert!(
            (one_octave - two_octaves - 12.0).abs() < 3.0,
            "HI rolled {} dB over the octave",
            one_octave - two_octaves
        );

        let flat_low = band_rms(80.0, |eq| eq.set_low(0.0));
        let killed_low = band_rms(80.0, |eq| eq.set_low(EQ_MIN_DB));
        assert!(
            killed_low < flat_low * 0.2,
            "80 Hz was not rolled off by LO"
        );
    }

    #[test]
    fn a_shelf_swept_the_other_way_boosts_its_band() {
        let flat = band_rms(80.0, |eq| eq.set_low(0.0));
        let boosted = band_rms(80.0, |eq| eq.set_low(EQ_MAX_DB));
        assert!(boosted > flat * 1.5, "full LO boost did not reach +6 dB");
    }

    #[test]
    fn each_bell_reaches_its_own_centre() {
        let bells: [(f32, fn(&mut Equalizer4, f32)); 2] = [
            (350.0, Equalizer4::set_low_mid),
            (2000.0, Equalizer4::set_high_mid),
        ];
        for (hz, set) in bells {
            let flat = band_rms(hz, |_| {});
            let cut = band_rms(hz, |eq| set(eq, -20.0));
            assert!(cut < flat * 0.5, "{hz} Hz was not cut by its own bell");
        }
    }

    #[test]
    fn the_mid_q_follows_its_table_and_holds_at_either_end() {
        let table = [(-10.0, 1.0), (0.0, 2.0)];
        assert_eq!(eq4_mid_q(&table, -20.0), 1.0);
        assert_eq!(eq4_mid_q(&table, -5.0), 1.5);
        assert_eq!(eq4_mid_q(&table, 0.0), 2.0);
        assert_eq!(eq4_mid_q(&table, 6.0), 2.0);
    }

    const REVERB_SR: f32 = 48_000.0;

    fn settled_reverb(tail_ms: f32) -> Reverb {
        let mut reverb = Reverb::new(REVERB_SR);
        reverb.set_tail_ms(tail_ms);
        reverb.settle();
        reverb
    }

    fn impulse_response(reverb: &mut Reverb, seconds: f32) -> Vec<f32> {
        let frames = (REVERB_SR * seconds) as usize;
        let mut out = Vec::with_capacity(frames);
        out.push(reverb.process(1.0, 1.0).0);
        for _ in 1..frames {
            out.push(reverb.process(0.0, 0.0).0);
        }
        out
    }

    fn level_db(response: &[f32], from_sec: f32, to_sec: f32) -> f32 {
        let window = &response[(REVERB_SR * from_sec) as usize..(REVERB_SR * to_sec) as usize];
        let power = window.iter().map(|sample| sample * sample).sum::<f32>() / window.len() as f32;
        10.0 * power.max(1e-30).log10()
    }

    #[test]
    fn a_reverb_with_nothing_sent_is_silent() {
        let mut reverb = settled_reverb(2_000.0);
        for _ in 0..REVERB_SR as usize {
            assert_eq!(reverb.process(0.0, 0.0), (0.0, 0.0));
        }
    }

    #[test]
    fn the_tail_falls_sixty_db_over_its_length() {
        let response = impulse_response(&mut settled_reverb(1_000.0), 1.3);
        let early = level_db(&response, 0.1, 0.2);
        let halfway = level_db(&response, 0.6, 0.7) - early;
        let past = level_db(&response, 1.1, 1.2) - early;
        assert!(
            (-45.0..-20.0).contains(&halfway),
            "halfway down {halfway} dB"
        );
        assert!(
            past < -45.0,
            "a tail of 1 s was only {past} dB down after it"
        );
    }

    #[test]
    fn a_longer_tail_rings_longer() {
        let short = impulse_response(&mut settled_reverb(500.0), 2.0);
        let long = impulse_response(&mut settled_reverb(3_000.0), 2.0);
        let late = |response: &[f32]| level_db(response, 1.0, 2.0) - level_db(response, 0.1, 0.2);
        assert!(
            late(&long) > late(&short) + 20.0,
            "{} against {}",
            late(&long),
            late(&short)
        );
    }

    fn steady_level_db(tail_ms: f32) -> f32 {
        let mut reverb = settled_reverb(tail_ms);
        let mut seed: u32 = 1;
        let mut low = 0.0f32;
        // Mostly below 1 kHz, where music carries its weight and damping takes little.
        let mut noise = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            low += 0.1 * ((seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0 - low);
            low
        };
        let (sent, returned): (Vec<f32>, Vec<f32>) = (0..(REVERB_SR * 3.0) as usize)
            .map(|_| {
                let sample = noise();
                (sample, reverb.process(sample, sample).0)
            })
            .unzip();
        level_db(&returned, 2.0, 3.0) - level_db(&sent, 2.0, 3.0)
    }

    #[test]
    fn the_tail_changes_how_long_it_rings_and_not_how_loud() {
        let short = steady_level_db(500.0);
        let long = steady_level_db(5_000.0);
        assert!((short - long).abs() < 3.0, "{short} dB against {long} dB");
        assert!(
            (-6.0..3.0).contains(&long),
            "a full send returned at {long} dB"
        );
    }

    #[test]
    fn a_tail_at_or_below_zero_still_dies() {
        let response = impulse_response(&mut settled_reverb(-100.0), 1.0);
        let half = (REVERB_SR * 0.5) as usize;
        assert!(response[half..].iter().all(|sample| sample.abs() < 1e-6));
    }

    #[test]
    fn the_left_and_right_tails_differ() {
        let mut reverb = settled_reverb(2_000.0);
        reverb.process(1.0, 1.0);
        let differs = (0..(REVERB_SR * 0.2) as usize)
            .map(|_| reverb.process(0.0, 0.0))
            .any(|(l, r)| (l - r).abs() > 1e-4);
        assert!(differs);
    }

    #[test]
    fn the_ring_out_lasts_at_least_the_tail() {
        assert!(settled_reverb(2_000.0).ring_out_frames() >= (REVERB_SR * 2.0) as usize);
    }

    #[test]
    fn limiter_no_reduction_below_threshold() {
        let mut lim = Limiter::new(44100.0);
        let (l, r) = lim.process(0.5, -0.3);
        assert!((l - 0.5).abs() < 1e-5, "l should be unchanged");
        assert!((r + 0.3).abs() < 1e-5, "r should be unchanged");
        assert!((lim.gain_reduction - 1.0).abs() < 1e-5);
    }

    #[test]
    fn limiter_instantaneous_attack_prevents_clipping() {
        let mut lim = Limiter::new(44100.0);
        let (l, r) = lim.process(2.0, 1.5);
        assert!(l.abs() <= 1.0, "l={l} exceeds 1.0");
        assert!(r.abs() <= 1.0, "r={r} exceeds 1.0");
        assert!(
            (l - Limiter::THRESHOLD).abs() < 1e-4,
            "peak should be at threshold"
        );
    }

    #[test]
    fn limiter_brickwall_over_many_samples() {
        let mut lim = Limiter::new(44100.0);
        let samples = [2.0f32, 1.5, 0.3, 1.8, -2.2, 0.8, 1.1, -1.05, 0.0, 1.6];
        for &s in &samples {
            let (l, r) = lim.process(s, -s * 0.7);
            assert!(l.abs() <= 1.0, "l={l} from input {s}");
            assert!(r.abs() <= 1.0, "r={r} from input {s}");
        }
    }

    #[test]
    fn limiter_releases_after_loud_burst() {
        let mut lim = Limiter::new(44100.0);
        lim.process(3.0, 0.0);
        let gr_after_burst = lim.gain_reduction;
        assert!(
            gr_after_burst < 0.5,
            "gain_reduction should be low after 3.0 input"
        );
        for _ in 0..44100 {
            lim.process(0.0, 0.0);
        }
        assert!(
            lim.gain_reduction > 0.99,
            "gain_reduction={} should be near 1.0 after 1s of silence",
            lim.gain_reduction
        );
    }

    #[test]
    fn the_filter_knob_settles_on_the_same_value_from_either_side() {
        let settled = |start: f32| {
            let mut filter = Filter::new(44100.0);
            filter.set_knob(start);
            for _ in 0..44100 {
                filter.process(0.0, 0.0);
            }
            filter.set_knob(0.5);
            for _ in 0..44100 {
                filter.process(0.0, 0.0);
            }
            filter.current_knob
        };
        assert_eq!(settled(-1.0), settled(1.0));
    }

    #[test]
    fn a_bypassed_filter_passes_the_dry_signal_unchanged() {
        let mut filter = Filter::new(44100.0);
        filter.set_knob(0.5);
        filter.set_active(true);
        for _ in 0..44100 {
            filter.process(0.25, -0.25);
        }
        filter.set_active(false);
        for _ in 0..(44100 * 2) {
            filter.process(0.25, -0.25);
        }
        assert_eq!(filter.process(0.3, -0.4), (0.3, -0.4));
    }

    #[test]
    fn a_released_limiter_returns_to_exactly_unity() {
        let mut lim = Limiter::new(44100.0);
        lim.process(3.0, 3.0);
        for _ in 0..(44100 * 4) {
            lim.process(0.0, 0.0);
        }
        assert_eq!(lim.gain_reduction, 1.0);
    }

    #[test]
    fn limiter_gain_reduction_stable_on_steady_signal() {
        let mut lim = Limiter::new(44100.0);
        for _ in 0..1000 {
            lim.process(1.5, 0.0);
        }
        let gr = lim.gain_reduction;
        lim.process(1.5, 0.0);
        assert!(
            (lim.gain_reduction - gr).abs() < 1e-6,
            "gain_reduction should be stable"
        );
    }
}

// Identity tests. The property tests above still pass after a change to a Q, a
// shelf frequency, or a reordered cascade. These pin the actual output.
#[cfg(test)]
mod identity {
    use super::tests::classic_eq4;
    use super::*;

    const SAMPLE_RATE: f32 = 44_100.0;
    const FRAMES: usize = 8192;
    const PROBE_STRIDE: usize = 97;
    // Loose enough to survive libm differences between architectures.
    const EPSILON: f32 = 1e-6;

    // Integer arithmetic only, so the stimulus is bit-identical everywhere.
    fn noise(count: usize) -> Vec<f32> {
        let mut state: u32 = 0x9E37_79B9;
        (0..count)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((state >> 8) as f32 / 8_388_608.0 - 1.0) * 0.5
            })
            .collect()
    }

    fn probe<F: FnMut(f32, f32) -> (f32, f32)>(mut process: F) -> Vec<f32> {
        let left = noise(FRAMES);
        let right = noise(FRAMES + 1);
        let mut out = Vec::new();
        for frame in 0..FRAMES {
            let (l, r) = process(left[frame], right[frame + 1]);
            if frame % PROBE_STRIDE == 0 {
                out.push(l);
                out.push(r);
            }
        }
        out
    }

    fn assert_matches(name: &str, actual: &[f32], expected: &[f32]) {
        assert_eq!(actual.len(), expected.len(), "{name}: probe count changed");
        for (index, (got, want)) in actual.iter().zip(expected).enumerate() {
            assert!(
                (got - want).abs() <= EPSILON,
                "{name}: probe {index} {got:e} != {want:e}"
            );
        }
    }

    fn eq_probes() -> Vec<f32> {
        let mut eq = classic_eq4(SAMPLE_RATE);
        eq.set_low(4.0);
        eq.set_low_mid(-8.0);
        eq.set_high_mid(3.0);
        eq.set_high(-12.0);
        probe(|l, r| eq.process(l, r))
    }

    // Swept, not fixed: the knob smoothing, coefficient refresh and kill-gain
    // ramp only run while the knob is moving.
    fn filter_probes() -> Vec<f32> {
        let mut filter = Filter::new(SAMPLE_RATE);
        filter.set_active(true);
        let mut frame = 0usize;
        probe(|l, r| {
            filter.set_knob(-1.0 + 2.0 * (frame as f32 / FRAMES as f32));
            frame += 1;
            filter.process(l, r)
        })
    }

    fn limiter_probes() -> Vec<f32> {
        let mut limiter = Limiter::new(SAMPLE_RATE);
        let mut frame = 0usize;
        probe(|l, r| {
            let boost = if (FRAMES / 3..FRAMES / 2).contains(&frame) {
                6.0
            } else {
                1.0
            };
            frame += 1;
            limiter.process(l * boost, r * boost)
        })
    }

    #[rustfmt::skip]
    const EXPECTED_EQ: &[f32] = &[
        -1.324709952e-1, 2.276586741e-1, -5.433481187e-2, -2.379948050e-1,
        6.471173465e-2, 1.574465036e-1, 1.138727665e-1, 2.689629197e-1,
        1.271901280e-1, 2.500383556e-1, -1.550689042e-1, -1.019197851e-1,
        1.313577443e-1, 2.015821487e-1, -6.819458306e-2, 1.266416721e-2,
        2.596150935e-1, 3.030305356e-2, 2.310652584e-1, 2.115294635e-1,
        1.441571712e-1, 1.578044742e-1, -6.970494986e-3, 2.010465860e-1,
        3.005047441e-1, 2.570427954e-1, 1.757200062e-1, 2.626730800e-1,
        -1.439720094e-1, 6.137210876e-2, 1.960377395e-1, -2.334267795e-1,
        -1.274943799e-1, 1.202696115e-1, -1.268526018e-1, 2.166930437e-1,
        5.313330889e-2, 2.832038105e-1, 1.148962602e-1, -6.645712256e-2,
        -1.839477867e-1, 1.811905056e-1, -9.397349507e-2, 1.502274871e-1,
        1.285328418e-1, 2.401066273e-1, 7.862018049e-2, 2.544380426e-1,
        -2.241368294e-1, -1.057844087e-1, 8.765471727e-2, 2.593964338e-1,
        1.521619111e-1, 2.540917993e-1, 2.479580045e-1, -1.082621813e-1,
        -1.356182396e-1, -1.958851069e-1, -8.063078672e-2, -1.596998423e-2,
        1.214292049e-1, -3.255382180e-3, 8.150199801e-2, -9.668639302e-2,
        -2.198381871e-1, 2.052933872e-1, 2.120500356e-1, 2.725176811e-1,
        1.410623491e-1, -4.675435275e-2, -2.169142663e-1, 8.377179503e-3,
        -2.959627286e-2, -2.856968939e-1, 8.358162642e-2, -5.843140185e-2,
        1.673666835e-1, -3.346149921e-1, -2.608200312e-1, -1.147356629e-1,
        -2.984780669e-1, -2.186537683e-1, -4.836743698e-3, 2.451847941e-1,
        1.648519784e-1, 1.072576940e-1, 3.041353077e-2, 4.660047591e-2,
        -2.255350053e-1, -9.727965295e-2, 1.647041366e-2, 1.858751178e-1,
        1.753129065e-3, 2.507622540e-2, -2.204872072e-1, 1.709071696e-1,
        -1.545617729e-1, -1.106580049e-1, -1.326831281e-1, -1.656599939e-1,
        1.592531055e-1, -1.667588651e-1, -1.415540129e-1, 1.287261099e-1,
        1.471493691e-1, 2.605446801e-2, -3.282814026e-1, -8.542001247e-2,
        -1.457607001e-1, -2.035155445e-1, 1.696975827e-1, -6.341329217e-2,
        -8.633554727e-2, 1.219843850e-1, -3.755034506e-2, 1.585929692e-1,
        -2.014648616e-1, 2.014262974e-1, 2.003653646e-1, -2.982605696e-1,
        -8.676282316e-2, 2.003021985e-1, -2.584623694e-1, -3.016157746e-1,
        -1.377993822e-2, 1.736326516e-1, 1.773674488e-1, 2.430833578e-1,
        9.681995213e-2, 3.630496562e-2, -4.581117630e-2, -2.483896911e-1,
        -5.295423418e-2, -2.060673535e-1, 1.960297227e-1, -2.997342944e-1,
        -2.315933108e-1, 2.480821013e-1, -3.156897724e-1, -3.158527017e-1,
        -2.024295330e-1, 8.973081410e-2, -1.191698313e-1, 2.377606928e-1,
        -7.887901366e-2, 1.930376142e-1, 1.721484959e-2, -1.215685979e-1,
        1.906346530e-1, 5.894189700e-2, 2.603484690e-1, 5.236881971e-2,
        1.129211336e-1, 2.876224741e-2, 2.012480199e-1, -4.520867765e-2,
        2.515160739e-1, -6.503134966e-2, -7.599392533e-2, -1.303970367e-1,
        8.108679950e-2, -4.619568586e-3, -1.347905248e-1, -1.542650610e-1,
        -2.541981936e-1, -1.510358751e-1, 7.969618589e-2, -2.269930765e-2,
        -6.818598509e-2, 1.188368946e-1,
    ];

    #[rustfmt::skip]
    const EXPECTED_FILTER: &[f32] = &[
        -2.3900086e-1, 4.1073608e-1, -5.3773385e-2, -3.7897918e-1, 1.14993e-1, 2.6481003e-1,
        1.703645e-1, 3.853445e-1, 1.9287166e-1, 3.678383e-1, -2.4762104e-1, -1.4551827e-1,
        2.2356635e-1, 3.0885965e-1, -1.987054e-1, -5.219383e-2, 2.8244856e-1, -1.9541327e-2,
        2.336366e-1, 1.8515253e-1, 2.5175297e-1, 2.3904517e-1, 2.083377e-2, 2.3234016e-1,
        3.035066e-1, 2.240779e-1, 1.7113067e-1, 2.5114554e-1, -1.0640578e-1, 1.127155e-1,
        2.2947471e-1, -1.867512e-1, -9.335261e-2, 1.3547577e-1, -1.4502983e-1, 1.5817752e-1,
        4.4995937e-2, 2.2493283e-1, 1.4185967e-2, -1.3786554e-1, -1.148517e-1, 1.6709988e-1,
        -6.0954727e-2, 1.1496327e-1, 7.868039e-2, 1.5232351e-1, 2.4576481e-2, 1.2806702e-1,
        -1.0465256e-1, -2.0468993e-2, 3.67889e-2, 1.3628359e-1, 8.873594e-2, 1.4328706e-1,
        6.992728e-2, -1.2964292e-1, -5.2889485e-2, -7.63029e-2, -4.4813193e-4, 3.4869965e-2,
        -1.5070373e-2, -6.4622775e-2, -6.8850465e-2, -1.4340244e-1, -1.4831945e-1, 3.8684152e-2,
        1.3466023e-2, 2.0203717e-2, 1.5388864e-1, 6.0777154e-2, -2.3385982e-1, -1.5863554e-1,
        -1.2338738e-1, -2.0873858e-1, -8.299588e-2, -9.4702676e-2, 2.2546425e-3, -1.9279113e-1,
        -2.3323752e-1, -1.6462044e-1, -1.5723753e-1, -2.1227634e-1, 6.5344915e-2, 1.3303834e-1,
        -4.207961e-2, -2.7307961e-2, 9.132732e-2, -3.202663e-2, 1.2074692e-1, 4.4119246e-2,
        8.901907e-2, 1.8973932e-1, 3.6611352e-2, 7.017813e-2, -4.9547768e-1, 2.6026016e-1,
        -3.0180472e-1, -1.9798678e-1, -2.703057e-1, -2.5984746e-1, 3.0476105e-1, -2.9689723e-1,
        -3.8993478e-1, 1.2881035e-1, 3.816981e-1, 1.3657752e-1, -4.07259e-1, 5.99364e-2,
        -3.6504382e-1, -4.6093518e-1, 3.0171752e-1, -1.7840406e-1, -2.0453526e-1, 1.8751413e-1,
        -1.5772861e-1, 2.782624e-1, -2.6883718e-1, 4.3968898e-1, 4.450286e-1, -4.3250492e-1,
        -1.17774926e-1, 3.5103494e-1, -2.8811234e-1, -3.1618124e-1, 1.3478956e-1, 3.9144447e-1,
        3.100561e-1, 3.8946524e-1, 2.7848703e-1, 1.4473598e-1, -4.8811097e-2, -2.8024596e-1,
        3.7625508e-4, -1.74797e-1, 3.5875973e-1, -2.7824154e-1, -2.825716e-1, 3.0744308e-1,
        -1.1065855e-1, -8.5957065e-2, -5.523883e-2, 2.5662157e-1, -6.912453e-2, 2.9932147e-1,
        -2.5124815e-1, 3.326053e-2, -2.8367572e-2, -1.6786465e-1, 1.170634e-1, -1.1673854e-2,
        2.8435925e-1, 1.9082686e-2, 2.0167722e-1, 6.942606e-2, 4.4250492e-2, -1.4029334e-1,
        -1.454945e-1, -3.034289e-1, -7.414156e-2, -3.1050649e-2, 1.277407e-1, 1.3420306e-1,
        7.836442e-2, 1.1202082e-2, -1.14798605e-1, 3.1137697e-2, -7.433204e-2, -8.333906e-2,
        -3.1131253e-2, 7.3595084e-2,
    ];

    #[rustfmt::skip]
    const EXPECTED_LIMITER: &[f32] = &[
        -2.390008569e-1, 4.107360840e-1, -4.862630367e-2, -3.926811218e-1,
        1.133236289e-1, 2.827962637e-1, 1.935958862e-1, 4.398347139e-1,
        2.131221294e-1, 4.198842049e-1, -3.017506003e-1, -1.730459332e-1,
        2.937932014e-1, 4.057862759e-1, -2.780621052e-1, -7.676559687e-2,
        3.995846510e-1, -3.127497435e-2, 3.562223315e-1, 2.849200368e-1,
        3.633637428e-1, 3.437343240e-1, 6.162726879e-2, 4.047954082e-1,
        4.789210558e-1, 3.436681032e-1, 3.067644835e-1, 4.495064616e-1,
        -2.199594975e-1, 1.853513122e-1, 4.684587717e-1, -3.366590142e-1,
        -1.878007054e-1, 2.746083140e-1, -3.190147281e-1, 3.214011192e-1,
        7.670730352e-2, 4.751932621e-1, 1.881819963e-2, -3.331387639e-1,
        -2.461463809e-1, 4.344985485e-1, -1.798579097e-1, 2.640733719e-1,
        1.836975217e-1, 3.758313060e-1, 1.702901125e-1, 4.541661739e-1,
        -2.869751453e-1, -4.447591305e-2, 1.277518272e-1, 4.249777198e-1,
        2.171851397e-1, 3.856863976e-1, 2.529299855e-1, -3.848286867e-1,
        -3.114664555e-1, -3.974967003e-1, -9.319922328e-2, 1.531449705e-1,
        1.232491508e-1, -2.441036701e-1, 3.137293160e-1, -3.542066514e-1,
        -5.397154689e-1, 9.900000095e-1, 8.105338216e-1, 9.043799043e-1,
        6.421998739e-1, -4.437752441e-2, -5.361838937e-1, 2.409272194e-1,
        6.555620581e-2, -9.824466109e-1, 4.042745829e-1, -1.957780868e-1,
        9.704103470e-1, -9.190876484e-1, -6.722197533e-1, -1.670109332e-1,
        -7.963636518e-1, -4.172662199e-1, 5.968379229e-2, 9.583563209e-1,
        6.986238956e-1, 4.819593132e-1, -5.322414264e-2, -3.793923929e-2,
        -9.241072088e-2, -2.432942018e-2, -2.441320568e-2, 7.889356464e-2,
        1.774571836e-2, 2.608516999e-2, -1.884339899e-1, 9.897895157e-2,
        -1.175010949e-1, -7.708183676e-2, -1.076404303e-1, -1.034757867e-1,
        1.240307912e-1, -1.208303943e-1, -1.620605737e-1, 5.353479832e-2,
        1.589838862e-1, 5.586277321e-2, -1.964984089e-1, 4.931537434e-3,
        -1.561032832e-1, -2.009069771e-1, 1.397862434e-1, -7.538914680e-2,
        -8.452069759e-2, 9.450650960e-2, -6.669761240e-2, 1.367160231e-1,
        -1.249178797e-1, 2.158496231e-1, 2.113628089e-1, -2.275473475e-1,
        -6.773389131e-2, 1.800775826e-1, -1.511975229e-1, -1.718634367e-1,
        6.177761033e-2, 2.177831680e-1, 1.680418700e-1, 2.270814776e-1,
        1.754277498e-1, 9.404078126e-2, -5.143712834e-2, -2.193477452e-1,
        -8.813343942e-3, -1.435737014e-1, 2.541498840e-1, -2.475761771e-1,
        -2.710966766e-1, 2.232928574e-1, -2.374183536e-1, -2.326692641e-1,
        -1.733583510e-1, 1.261301488e-1, -1.236646920e-1, 2.523081601e-1,
        -1.823345572e-1, 1.165421233e-1, 3.368195146e-2, -1.438076049e-1,
        1.633087844e-1, 1.891231723e-2, 2.727175653e-1, 8.755733143e-4,
        2.051639706e-1, 7.725986838e-2, 1.445496678e-1, -1.325816959e-1,
        1.018263102e-1, -2.423454523e-1, -1.305276304e-1, -1.664938033e-1,
        2.048175782e-2, -4.271231219e-2, -9.538593888e-2, -1.182108521e-1,
        -2.213758379e-1, -8.635760099e-2, 1.521681398e-1, 2.183447592e-2,
        -1.000035107e-1, 1.091056019e-1,
    ];

    #[test]
    fn eq_output_is_unchanged() {
        assert_matches("eq", &eq_probes(), EXPECTED_EQ);
    }

    #[test]
    fn filter_sweep_output_is_unchanged() {
        assert_matches("filter", &filter_probes(), EXPECTED_FILTER);
    }

    #[test]
    fn limiter_output_is_unchanged() {
        assert_matches("limiter", &limiter_probes(), EXPECTED_LIMITER);
    }

    #[test]
    #[ignore = "generator, not a check: run with --ignored --nocapture to reprint the tables"]
    fn print_identity_tables() {
        for (name, values) in [
            ("EQ", eq_probes()),
            ("FILTER", filter_probes()),
            ("LIMITER", limiter_probes()),
        ] {
            println!("// {name}");
            for chunk in values.chunks(4) {
                let row: Vec<String> = chunk.iter().map(|v| format!("{v:.9e}")).collect();
                println!("        {},", row.join(", "));
            }
            println!("// end {name}");
        }
    }
}
