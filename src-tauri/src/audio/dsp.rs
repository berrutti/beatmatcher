// Audio EQ Cookbook biquad (Robert Bristow-Johnson), Direct Form II Transposed.

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
        self.delay1 = self.b1 * x - self.a1 * y + self.delay2;
        self.delay2 = self.b2 * x - self.a2 * y;
        y
    }

    // Replace the filter coefficients while preserving the delay-line state so
    // there is no discontinuity click on a live signal. Do NOT call this when
    // transitioning into identity (the dead zone): identity delay lines must be
    // zeroed, not carried over from an active filter.
    #[inline]
    pub(crate) fn set_coefficients(&mut self, src: Self) {
        let d1 = self.delay1;
        let d2 = self.delay2;
        *self = src;
        self.delay1 = d1;
        self.delay2 = d2;
    }

    pub(crate) fn low_shelf(sr: f32, freq: f32, db: f32) -> Self {
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

    pub(crate) fn peaking(sr: f32, freq: f32, q: f32, db: f32) -> Self {
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

    pub(crate) fn low_pass(sr: f32, freq: f32, q: f32) -> Self {
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

    pub(crate) fn high_pass(sr: f32, freq: f32, q: f32) -> Self {
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

    // Flat magnitude, phase only. A Linkwitz-Riley split sums to this, so a band skipping
    // one crossover needs the matching allpass to stay aligned with the ones that did not.
    pub(crate) fn all_pass(sr: f32, freq: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * freq / sr;
        let cos_w = w0.cos();
        let alpha = w0.sin() / (2.0 * q);

        let b0 = 1.0 - alpha;
        let b1 = -2.0 * cos_w;
        let b2 = 1.0 + alpha;
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

    pub(crate) fn high_shelf(sr: f32, freq: f32, db: f32) -> Self {
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

// Low/high: two cascaded 2nd-order shelves (4th-order, kills decisively); mid: one wide-Q peak.

const EQ_LOW_SHELF_HZ: f32 = 200.0;
const EQ_MID_PEAK_HZ: f32 = 1000.0;
const EQ_MID_Q: f32 = 0.4;
const EQ_HIGH_SHELF_HZ: f32 = 6_000.0;

pub(crate) struct EqState {
    sample_rate: f32,
    low_stage1: [Biquad; 2],
    low_stage2: [Biquad; 2],
    mid: [Biquad; 2],
    high_stage1: [Biquad; 2],
    high_stage2: [Biquad; 2],
}

impl EqState {
    pub(crate) fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            low_stage1: [Biquad::identity(), Biquad::identity()],
            low_stage2: [Biquad::identity(), Biquad::identity()],
            mid: [Biquad::identity(), Biquad::identity()],
            high_stage1: [Biquad::identity(), Biquad::identity()],
            high_stage2: [Biquad::identity(), Biquad::identity()],
        }
    }

    pub(crate) fn set_low(&mut self, db: f32) {
        let stage = Biquad::low_shelf(self.sample_rate, EQ_LOW_SHELF_HZ, db / 2.0);
        for (first, second) in self.low_stage1.iter_mut().zip(self.low_stage2.iter_mut()) {
            first.set_coefficients(stage);
            second.set_coefficients(stage);
        }
    }

    pub(crate) fn set_mid(&mut self, db: f32) {
        let filter = Biquad::peaking(self.sample_rate, EQ_MID_PEAK_HZ, EQ_MID_Q, db);
        for ch in &mut self.mid {
            ch.set_coefficients(filter);
        }
    }

    pub(crate) fn set_high(&mut self, db: f32) {
        let stage = Biquad::high_shelf(self.sample_rate, EQ_HIGH_SHELF_HZ, db / 2.0);
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
// Crossfade time for bypass toggle. The filter output is crossfaded with the
// dry signal so the knob position never sweeps during a bypass transition.
const FILTER_CROSSFADE_TAU_SEC: f32 = 0.05;
// Coefficients are refreshed every N samples (knob is already smoothed per-sample).
// Small enough to avoid click artifacts at high Q, large enough to keep CPU light.
const FILTER_COEFF_REFRESH_INTERVAL: u32 = 4;
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

pub(crate) struct FilterState {
    sample_rate: f32,
    target_knob: f32,
    current_knob: f32,
    smoothing_coeff: f32,
    coeff_refresh_counter: u32,
    // Two cascaded 2nd-order stages per channel give 4th-order (-24 dB/oct) rolloff.
    filters_a: [Biquad; 2],
    filters_b: [Biquad; 2],
    makeup: f32,
    // Dry/wet crossfade: 0.0 = fully filtered, 1.0 = fully dry (bypassed).
    // The filter always runs at its set position; bypass is a gain crossfade,
    // never a frequency sweep.
    crossfade: f32,
    crossfade_target: f32,
    crossfade_coeff: f32,
}

impl FilterState {
    pub(crate) fn new(sample_rate: f32) -> Self {
        let smoothing_coeff = 1.0 - (-1.0 / (sample_rate * FILTER_SMOOTHING_TAU_SEC)).exp();
        let crossfade_coeff = 1.0 - (-1.0 / (sample_rate * FILTER_CROSSFADE_TAU_SEC)).exp();
        Self {
            sample_rate,
            target_knob: 0.0,
            current_knob: 0.0,
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
    }

    pub(crate) fn set_active(&mut self, active: bool) {
        self.crossfade_target = if active { 0.0 } else { 1.0 };
    }

    fn update_filters(&mut self) {
        let knob = self.current_knob;
        let abs_knob = knob.abs();

        if abs_knob <= FILTER_CENTER_DEAD_ZONE {
            // Reset to identity with zeroed delay lines. Preserving delay lines here
            // would allow IIR state from the previous active filter to ring through,
            // causing transient overshoots that push samples above 1.0.
            let identity = Biquad::identity();
            for (a, b) in self.filters_a.iter_mut().zip(self.filters_b.iter_mut()) {
                *a = identity;
                *b = identity;
            }
            self.makeup = 1.0;
            return;
        }

        let sweep = (abs_knob - FILTER_CENTER_DEAD_ZONE) / (1.0 - FILTER_CENTER_DEAD_ZONE);
        let q = FILTER_CENTER_Q + (FILTER_RESONANCE_Q - FILTER_CENTER_Q) * sweep;
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
        self.current_knob += (self.target_knob - self.current_knob) * self.smoothing_coeff;

        if self.coeff_refresh_counter == 0 {
            self.update_filters();
        }
        self.coeff_refresh_counter =
            (self.coeff_refresh_counter + 1) % FILTER_COEFF_REFRESH_INTERVAL;

        // Kill gain: fade to 0 as the sweep enters the last 20% of its range so the
        // extreme position always reaches -infinity regardless of biquad slope.
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

        // Filter always runs at its set position. Bypass is a crossfade between
        // filtered and dry so no frequency sweep ever happens during a toggle.
        let gain = kill_gain * self.makeup;
        let l_filtered = self.filters_b[0].process(self.filters_a[0].process(l)) * gain;
        let r_filtered = self.filters_b[1].process(self.filters_a[1].process(r)) * gain;

        self.crossfade += (self.crossfade_target - self.crossfade) * self.crossfade_coeff;
        let entry = ((abs_knob - FILTER_CENTER_DEAD_ZONE) / FILTER_ENTRY_WIDTH).clamp(0.0, 1.0);
        let wet = (1.0 - self.crossfade) * entry;
        (
            l * (1.0 - wet) + l_filtered * wet,
            r * (1.0 - wet) + r_filtered * wet,
        )
    }
}

// True-peak brickwall: instantaneous attack (no sample exceeds THRESHOLD), ~150ms release.

pub(crate) struct LimiterState {
    pub(crate) gain_reduction: f32,
    release_coeff: f32,
}

impl LimiterState {
    pub(crate) const THRESHOLD: f32 = 0.99;
    const RELEASE_TAU_SEC: f32 = 0.150;

    pub(crate) fn new(sample_rate: f32) -> Self {
        Self {
            gain_reduction: 1.0,
            release_coeff: 1.0 - (-1.0 / (sample_rate * Self::RELEASE_TAU_SEC)).exp(),
        }
    }

    // Attack is instantaneous: gain_reduction jumps immediately to prevent any
    // sample from exceeding THRESHOLD. Release recovers smoothly over ~150ms.
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
            self.gain_reduction += (target_gr - self.gain_reduction) * self.release_coeff;
        }
        (
            (l * self.gain_reduction).clamp(-1.0, 1.0),
            (r * self.gain_reduction).clamp(-1.0, 1.0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_wave(freq_hz: f32, sample_rate: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq_hz * i as f32 / sample_rate).sin())
            .collect()
    }

    // --- Biquad ---

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

    // --- Biquad filter correctness ---

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

    // --- FilterState (regression for the clipping bug) ---

    // Before the fix, FilterState at knob=0 placed a 20 Hz HPF whose IIR delay
    // lines accumulated state that caused per-sample outputs exceeding 1.0 on
    // transients. Now it uses identity, which has L1 norm = 1.
    #[test]
    fn filter_center_impulse_never_clips() {
        let mut state = FilterState::new(44100.0);
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
        let mut state = FilterState::new(sr);
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
            let mut state = FilterState::new(sr);
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
            let mut state = FilterState::new(sr);
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
        let mut state = FilterState::new(sr);
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
            let mut state = FilterState::new(sr);
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

    // --- EqState ---

    #[test]
    fn eq_at_zero_db_is_transparent() {
        let sr = 44100.0f32;
        let mut eq = EqState::new(sr);
        for &s in sine_wave(1000.0, sr, 1024).iter() {
            let (l, _r) = eq.process(s, s);
            assert!((l - s).abs() < 1e-6, "input={} output={}", s, l);
        }
    }

    fn rms(signal: &[f32]) -> f32 {
        let sum_sq: f32 = signal.iter().map(|&x| x * x).sum();
        (sum_sq / signal.len() as f32).sqrt()
    }

    #[test]
    fn eq_high_cut_reduces_high_frequency_level() {
        let sr = 44100.0f32;
        let input: Vec<f32> = (0..8192)
            .map(|i| (2.0 * std::f32::consts::PI * 8000.0 * i as f32 / sr).sin())
            .collect();

        let mut flat = EqState::new(sr);
        let flat_out: Vec<f32> = input.iter().map(|&s| flat.process(s, s).0).collect();

        let mut cut = EqState::new(sr);
        cut.set_high(-12.0);
        let cut_out: Vec<f32> = input.iter().map(|&s| cut.process(s, s).0).collect();

        let flat_rms = rms(&flat_out[4096..]);
        let cut_rms = rms(&cut_out[4096..]);
        assert!(
            cut_rms < flat_rms * 0.6,
            "high cut should reduce 8kHz level; flat_rms={:.4} cut_rms={:.4}",
            flat_rms,
            cut_rms
        );
    }

    #[test]
    fn eq_low_cut_reduces_low_frequency_level() {
        let sr = 44100.0f32;
        let input: Vec<f32> = (0..8192)
            .map(|i| (2.0 * std::f32::consts::PI * 80.0 * i as f32 / sr).sin())
            .collect();

        let mut flat = EqState::new(sr);
        let flat_out: Vec<f32> = input.iter().map(|&s| flat.process(s, s).0).collect();

        let mut cut = EqState::new(sr);
        cut.set_low(-12.0);
        let cut_out: Vec<f32> = input.iter().map(|&s| cut.process(s, s).0).collect();

        let flat_rms = rms(&flat_out[4096..]);
        let cut_rms = rms(&cut_out[4096..]);
        assert!(
            cut_rms < flat_rms * 0.6,
            "low cut should reduce 80Hz level; flat_rms={:.4} cut_rms={:.4}",
            flat_rms,
            cut_rms
        );
    }

    // --- LimiterState ---

    #[test]
    fn limiter_no_reduction_below_threshold() {
        let mut lim = LimiterState::new(44100.0);
        let (l, r) = lim.process(0.5, -0.3);
        assert!((l - 0.5).abs() < 1e-5, "l should be unchanged");
        assert!((r + 0.3).abs() < 1e-5, "r should be unchanged");
        assert!((lim.gain_reduction - 1.0).abs() < 1e-5);
    }

    #[test]
    fn limiter_instantaneous_attack_prevents_clipping() {
        let mut lim = LimiterState::new(44100.0);
        // First sample is loud. Must be limited in the same sample, not the next one.
        let (l, r) = lim.process(2.0, 1.5);
        assert!(l.abs() <= 1.0, "l={l} exceeds 1.0");
        assert!(r.abs() <= 1.0, "r={r} exceeds 1.0");
        assert!(
            (l - LimiterState::THRESHOLD).abs() < 1e-4,
            "peak should be at threshold"
        );
    }

    #[test]
    fn limiter_brickwall_over_many_samples() {
        let mut lim = LimiterState::new(44100.0);
        let samples = [2.0f32, 1.5, 0.3, 1.8, -2.2, 0.8, 1.1, -1.05, 0.0, 1.6];
        for &s in &samples {
            let (l, r) = lim.process(s, -s * 0.7);
            assert!(l.abs() <= 1.0, "l={l} from input {s}");
            assert!(r.abs() <= 1.0, "r={r} from input {s}");
        }
    }

    #[test]
    fn limiter_releases_after_loud_burst() {
        let mut lim = LimiterState::new(44100.0);
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
    fn limiter_gain_reduction_stable_on_steady_signal() {
        let mut lim = LimiterState::new(44100.0);
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
// shelf frequency, or a reordered cascade; these pin the actual output.
#[cfg(test)]
mod identity {
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
        let mut eq = EqState::new(SAMPLE_RATE);
        eq.set_low(6.0);
        eq.set_mid(-8.0);
        eq.set_high(3.0);
        probe(|l, r| eq.process(l, r))
    }

    // Swept, not fixed: the knob smoothing, coefficient refresh and kill-gain
    // ramp only run while the knob is moving.
    fn filter_probes() -> Vec<f32> {
        let mut filter = FilterState::new(SAMPLE_RATE);
        filter.set_active(true);
        let mut frame = 0usize;
        probe(|l, r| {
            filter.set_knob(-1.0 + 2.0 * (frame as f32 / FRAMES as f32));
            frame += 1;
            filter.process(l, r)
        })
    }

    fn limiter_probes() -> Vec<f32> {
        let mut limiter = LimiterState::new(SAMPLE_RATE);
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
        -2.672801912e-1, 4.593356848e-1, -3.725598380e-2, -4.196695983e-1,
        1.208134592e-1, 2.411441356e-1, 2.271278203e-2, 3.183583915e-1,
        2.303895205e-1, 4.260113239e-1, -2.639633715e-1, -4.099877179e-2,
        2.332159281e-1, 2.598359883e-1, -1.492358148e-1, 1.026818752e-1,
        4.079803824e-1, -1.939572394e-1, 4.020574987e-1, 2.229078710e-1,
        5.408157110e-1, 2.583176792e-1, -6.220184267e-2, 3.504433930e-1,
        5.834364891e-1, 2.522757947e-1, 3.819344044e-1, 4.356383085e-1,
        -1.164105386e-1, 3.443693221e-1, 6.957352757e-1, -5.202770233e-1,
        -2.487626374e-1, 3.527850807e-1, -3.361545801e-1, 5.375764370e-1,
        -4.841346294e-2, 4.384179413e-1, -2.171876431e-1, -4.379880428e-1,
        -2.719421983e-1, 5.747131705e-1, -3.542903066e-1, 2.982466221e-1,
        3.401397765e-1, 4.098565280e-1, 1.335864514e-1, 3.936891258e-1,
        -1.770126820e-1, 1.101755649e-1, -4.605134949e-2, 3.754656613e-1,
        3.960167766e-1, 4.448432326e-1, 1.589553654e-1, -5.938205719e-1,
        -2.785788774e-1, -2.360838354e-1, -7.050324231e-2, 8.473712206e-2,
        1.837446988e-1, -8.268839121e-2, 3.766070586e-3, -3.738295436e-1,
        -2.435549200e-1, 6.596841216e-1, 1.941982061e-1, 1.599377990e-1,
        5.479906797e-1, -8.527469635e-2, -2.239497304e-1, 2.501289845e-1,
        -2.334518433e-1, -6.923208237e-1, 2.363508195e-1, -2.286707908e-1,
        8.191981912e-1, -6.429304481e-1, -4.579300284e-1, -1.051228307e-2,
        -2.922110558e-1, -2.574790642e-2, 6.361593306e-2, 4.918230176e-1,
        4.798528850e-1, 1.432533562e-1, -1.627066135e-1, -3.348079324e-2,
        -4.448823035e-1, -3.996755183e-2, -1.942113340e-1, 2.606572509e-1,
        3.230783343e-2, 4.009339958e-2, -7.709110379e-1, 3.742244244e-1,
        -4.105801880e-1, -1.336861849e-1, -8.345796168e-2, -5.721709132e-2,
        3.655659854e-1, -4.698076844e-1, -6.786015034e-1, 2.255944312e-1,
        3.810105920e-1, -5.779450387e-2, -6.122059822e-1, 1.143168285e-1,
        -6.168531179e-1, -4.264423847e-1, 1.667549014e-1, -3.931438029e-1,
        -2.653994858e-1, 2.894300818e-1, 8.414694667e-2, 5.003638268e-1,
        -3.312206268e-1, 6.116352677e-1, 7.722671032e-1, -6.148427725e-1,
        -1.651841849e-1, 4.627656043e-1, -2.475629300e-1, -2.161644697e-1,
        2.331183702e-1, 4.536537528e-1, 4.082171023e-1, 3.329514861e-1,
        3.762441278e-1, 2.967011929e-2, -1.990038157e-1, -4.663297832e-1,
        1.335901916e-1, -2.458026409e-1, 6.261838675e-1, -7.167561054e-1,
        -4.929068089e-1, 7.193861604e-1, -3.665972352e-1, -2.730312943e-1,
        -3.934606314e-1, 3.216216862e-1, -4.513016343e-2, 7.001420856e-1,
        -5.711373687e-1, 2.935391366e-1, -8.487974107e-2, -3.551476002e-1,
        3.019623458e-1, -5.657370016e-2, 4.645871222e-1, -1.844502091e-1,
        1.856991053e-1, -8.334764838e-2, 9.941054881e-2, -4.191124439e-1,
        2.643807232e-1, -4.186331630e-1, -1.599213183e-1, -1.983188093e-1,
        3.682218492e-2, -9.008228779e-2, 7.181882858e-3, -8.033190668e-2,
        -3.543400764e-1, -6.204452366e-3, 2.286418229e-1, -8.282564580e-2,
        -1.612312496e-1, 2.441443801e-1,
    ];

    #[rustfmt::skip]
    const EXPECTED_FILTER: &[f32] = &[
        -2.390008569e-1, 4.107360840e-1, -5.393578112e-2, -3.787913322e-1,
        1.148596406e-1, 2.643350959e-1, 1.704293936e-1, 3.851973712e-1,
        1.929397285e-1, 3.682322502e-1, -2.479301393e-1, -1.459030360e-1,
        2.233161479e-1, 3.085779548e-1, -1.991903633e-1, -5.258113891e-2,
        2.827291787e-1, -1.941110566e-2, 2.333846092e-1, 1.849579513e-1,
        2.518873811e-1, 2.390856743e-1, 2.078615688e-2, 2.322865129e-1,
        3.035451472e-1, 2.241833657e-1, 1.710699946e-1, 2.510573864e-1,
        -1.063671261e-1, 1.127645522e-1, 2.294631153e-1, -1.867587566e-1,
        -9.337446094e-2, 1.354653835e-1, -1.450302005e-1, 1.581830382e-1,
        4.503925517e-2, 2.249642313e-1, 1.409328729e-2, -1.379725337e-1,
        -1.148145497e-1, 1.672126055e-1, -6.092323363e-2, 1.148921698e-1,
        7.857815921e-2, 1.521856785e-1, 2.457335964e-2, 1.280554533e-1,
        -1.046603769e-1, -2.038645744e-2, 3.679289296e-2, 1.363310665e-1,
        8.867676556e-2, 1.430531442e-1, 7.027553022e-2, -1.290417016e-1,
        -5.326759443e-2, -7.688391954e-2, -4.984419793e-4, 3.461273015e-2,
        -1.476891711e-2, -6.440655887e-2, -6.913225353e-2, -1.435296088e-1,
        -1.487194896e-1, 3.819450736e-2, 1.412918419e-2, 2.109184116e-2,
        1.543891430e-1, 6.110650674e-2, -2.329817563e-1, -1.582662612e-1,
        -1.236121953e-1, -2.095370889e-1, -8.379723877e-2, -9.530526400e-2,
        2.261012793e-3, -1.924462765e-1, -2.332431674e-1, -1.647243798e-1,
        -1.564071476e-1, -2.119701952e-1, 6.396692991e-2, 1.333293021e-1,
        -4.278868809e-2, -2.724905685e-2, 9.190837294e-2, -3.080812469e-2,
        1.190198734e-1, 4.614865035e-2, 8.873917162e-2, 1.886469126e-1,
        3.620712459e-2, 7.047310472e-2, -4.954776764e-1, 2.602601647e-1,
        -3.018047214e-1, -1.979867816e-1, -2.703056931e-1, -2.598474622e-1,
        3.047610521e-1, -2.968972325e-1, -3.899347782e-1, 1.288103461e-1,
        3.816831708e-1, 1.365909129e-1, -4.073510170e-1, 5.990144610e-2,
        -3.650510013e-1, -4.610029161e-1, 3.016918898e-1, -1.785205454e-1,
        -2.046041936e-1, 1.874401420e-1, -1.577393264e-1, 2.782109380e-1,
        -2.690242529e-1, 4.398821294e-1, 4.453959465e-1, -4.328653812e-1,
        -1.177528426e-1, 3.509778380e-1, -2.882046103e-1, -3.161724508e-1,
        1.347891390e-1, 3.917140365e-1, 3.104566038e-1, 3.903178871e-1,
        2.783094943e-1, 1.445505917e-1, -4.857312143e-2, -2.801694274e-1,
        3.074542037e-4, -1.751447320e-1, 3.594020605e-1, -2.784583569e-1,
        -2.823804319e-1, 3.074039519e-1, -1.109221950e-1, -8.632538468e-2,
        -5.524119735e-2, 2.567976117e-1, -6.930411607e-2, 2.996577024e-1,
        -2.508986294e-1, 3.323195130e-2, -2.918217331e-2, -1.689103991e-1,
        1.174597889e-1, -1.151192933e-2, 2.855313420e-1, 1.972979121e-2,
        2.017346919e-1, 7.033336163e-2, 4.450906068e-2, -1.396940053e-1,
        -1.452449411e-1, -3.044017255e-1, -7.435798645e-2, -3.079995327e-2,
        1.267509758e-1, 1.345119625e-1, 7.836415619e-2, 1.121155731e-2,
        -1.157617792e-1, 3.014750034e-2, -7.453708351e-2, -8.478327841e-2,
        -3.130339086e-2, 7.391124219e-2,
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
