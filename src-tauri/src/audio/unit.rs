use super::dsp::{Biquad, EqState, FilterState};

const GAIN_SMOOTHING_TAU_SEC: f32 = 0.010;

pub trait AudioUnit: Send {
    fn set_param(&mut self, param: &str, value: f32);

    /// The value last set, not the smoothed value the DSP is currently at.
    fn param(&self, param: &str) -> Option<f32>;

    fn process(&mut self, l: f32, r: f32) -> (f32, f32);

    // Session-view mute, audition-only and never logged, so it stays off the param path and
    // out of `.bms` events. The strip forwards it to the unit filling the fader role.
    fn set_muted(&mut self, _muted: bool) {}

    // Master scope, so it arrives outside the strip's own param space. The strip
    // forwards it to the unit filling the fader role.
    fn set_fader_curve(&mut self, _curve: session_core::FaderCurve) {}
}

struct Eq3Band {
    eq: EqState,
    low: f32,
    mid: f32,
    high: f32,
}

impl AudioUnit for Eq3Band {
    fn set_param(&mut self, param: &str, value: f32) {
        match param {
            "low" => {
                self.low = value;
                self.eq.set_low(value);
            }
            "mid" => {
                self.mid = value;
                self.eq.set_mid(value);
            }
            "high" => {
                self.high = value;
                self.eq.set_high(value);
            }
            _ => {}
        }
    }

    fn param(&self, param: &str) -> Option<f32> {
        match param {
            "low" => Some(self.low),
            "mid" => Some(self.mid),
            "high" => Some(self.high),
            _ => None,
        }
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.eq.process(l, r)
    }
}

struct SweepFilter {
    filter: FilterState,
    knob: f32,
    active: bool,
}

impl AudioUnit for SweepFilter {
    fn set_param(&mut self, param: &str, value: f32) {
        match param {
            "value" => {
                self.knob = value;
                self.filter.set_knob(value);
            }
            "active" => {
                self.active = value != 0.0;
                self.filter.set_active(self.active);
            }
            _ => {}
        }
    }

    fn param(&self, param: &str) -> Option<f32> {
        match param {
            "value" => Some(self.knob),
            "active" => Some(if self.active { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.filter.process(l, r)
    }
}

struct Fader {
    position: f32,
    curve: session_core::FaderCurve,
    target_gain: f32,
    current_gain: f32,
    smooth_coeff: f32,
    muted: bool,
    mute_gain: f32,
}

impl Fader {
    // Curved before smoothing, so the one-pole interpolates what is heard.
    fn resolve_gain(&mut self) {
        self.target_gain = self.curve.gain(f64::from(self.position)) as f32;
    }
}

impl AudioUnit for Fader {
    fn set_param(&mut self, param: &str, value: f32) {
        if param == "gain" {
            self.position = value.clamp(0.0, 1.0);
            self.resolve_gain();
        }
    }

    /// The throw, not the gain it curves to: this is the value a `.bms` records.
    fn param(&self, param: &str) -> Option<f32> {
        (param == "gain").then_some(self.position)
    }

    fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    fn set_fader_curve(&mut self, curve: session_core::FaderCurve) {
        self.curve = curve;
        self.resolve_gain();
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.current_gain = approach(self.current_gain, self.target_gain, self.smooth_coeff);
        // Mute fades over the same time constant as the fader to avoid clicks.
        let mute_target = if self.muted { 0.0 } else { 1.0 };
        self.mute_gain = approach(self.mute_gain, mute_target, self.smooth_coeff);
        let gain = self.current_gain * self.mute_gain;
        (l * gain, r * gain)
    }
}

/// A one-pole in f32 stops moving while still short of its target, at a point that depends
/// on where it came from, so two mixers on the same value settle on different gains.
#[inline]
pub(crate) fn approach(current: f32, target: f32, coeff: f32) -> f32 {
    let next = current + (target - current) * coeff;
    if next == current {
        target
    } else {
        next
    }
}

// Linkwitz-Riley 4th order: two cascaded Butterworth sections per band edge.
const ISOLATOR_LOW_HZ: f32 = 300.0;
const ISOLATOR_HIGH_HZ: f32 = 3_000.0;
const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

// A two-stage Linkwitz-Riley tree per channel: split at the low crossover,
// then split what is left at the high one.
struct IsolatorChannel {
    low_split_lp: [Biquad; 2],
    low_split_hp: [Biquad; 2],
    high_split_lp: [Biquad; 2],
    high_split_hp: [Biquad; 2],
    // The low band skips the second crossover, so without this it lands out of phase with
    // the two bands that went through it and the sum notches.
    low_align: Biquad,
}

impl IsolatorChannel {
    fn new(sample_rate: f32) -> Self {
        Self {
            low_split_lp: [Biquad::low_pass(sample_rate, ISOLATOR_LOW_HZ, BUTTERWORTH_Q); 2],
            low_split_hp: [Biquad::high_pass(sample_rate, ISOLATOR_LOW_HZ, BUTTERWORTH_Q); 2],
            high_split_lp: [Biquad::low_pass(sample_rate, ISOLATOR_HIGH_HZ, BUTTERWORTH_Q); 2],
            high_split_hp: [Biquad::high_pass(sample_rate, ISOLATOR_HIGH_HZ, BUTTERWORTH_Q); 2],
            low_align: Biquad::all_pass(sample_rate, ISOLATOR_HIGH_HZ, BUTTERWORTH_Q),
        }
    }

    #[inline]
    fn process(&mut self, x: f32, gains: &[f32; 3]) -> f32 {
        let low = self.low_split_lp[0].process(x);
        let low = self.low_split_lp[1].process(low);
        let low = self.low_align.process(low);

        let upper = self.low_split_hp[0].process(x);
        let upper = self.low_split_hp[1].process(upper);

        let mid = self.high_split_lp[0].process(upper);
        let mid = self.high_split_lp[1].process(mid);

        let high = self.high_split_hp[0].process(upper);
        let high = self.high_split_hp[1].process(high);

        low * gains[0] + mid * gains[1] + high * gains[2]
    }
}

struct Isolator3Band {
    channels: [IsolatorChannel; 2],
    targets: [f32; 3],
    gains: [f32; 3],
    smooth_coeff: f32,
}

impl AudioUnit for Isolator3Band {
    fn set_param(&mut self, param: &str, value: f32) {
        let band = match param {
            "low" => 0,
            "mid" => 1,
            "high" => 2,
            _ => return,
        };
        self.targets[band] = value.clamp(0.0, 1.0);
    }

    fn param(&self, param: &str) -> Option<f32> {
        match param {
            "low" => Some(self.targets[0]),
            "mid" => Some(self.targets[1]),
            "high" => Some(self.targets[2]),
            _ => None,
        }
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        for (gain, target) in self.gains.iter_mut().zip(self.targets) {
            *gain = approach(*gain, target, self.smooth_coeff);
        }
        (
            self.channels[0].process(l, &self.gains),
            self.channels[1].process(r, &self.gains),
        )
    }
}

pub fn make_unit(unit_id: &str, sample_rate: f32) -> Option<Box<dyn AudioUnit>> {
    match unit_id {
        "eq3band" => Some(Box::new(Eq3Band {
            eq: EqState::new(sample_rate),
            low: 0.0,
            mid: 0.0,
            high: 0.0,
        })),
        "sweep_filter" => Some(Box::new(SweepFilter {
            filter: FilterState::new(sample_rate),
            knob: 0.0,
            active: false,
        })),
        "isolator3band" => Some(Box::new(Isolator3Band {
            channels: [
                IsolatorChannel::new(sample_rate),
                IsolatorChannel::new(sample_rate),
            ],
            targets: [1.0; 3],
            gains: [1.0; 3],
            smooth_coeff: 1.0 - (-1.0 / (sample_rate * GAIN_SMOOTHING_TAU_SEC)).exp(),
        })),
        "fader" => Some(Box::new(Fader {
            position: 1.0,
            curve: session_core::FaderCurve::default(),
            target_gain: 1.0,
            current_gain: 1.0,
            smooth_coeff: 1.0 - (-1.0 / (sample_rate * GAIN_SMOOTHING_TAU_SEC)).exp(),
            muted: false,
            mute_gain: 1.0,
        })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use session_core::{ParamScope, CLASSIC_3BAND};

    #[test]
    fn every_slot_in_every_manifest_has_a_unit() {
        for manifest in session_core::MANIFESTS {
            for slot in manifest.strip {
                assert!(
                    make_unit(slot.unit_id, 44100.0).is_some(),
                    "no unit '{}' for slot '{}' in '{}'",
                    slot.unit_id,
                    slot.slot,
                    manifest.id
                );
            }
        }
    }

    const SAMPLE_RATE: f32 = 44100.0;
    const PROBE_FRAMES: usize = 4410;

    // One tone per EQ band (60 Hz, 1 kHz, 10 kHz) against the 200 Hz shelf, 1 kHz peak and
    // 6 kHz shelf, so cutting any single band shows up in the total.
    fn rms_through(unit: &mut dyn AudioUnit) -> f32 {
        let mut sum = 0.0;
        for frame in 0..PROBE_FRAMES {
            let phase = frame as f32 / SAMPLE_RATE;
            let tone = |hz: f32| (std::f32::consts::TAU * hz * phase).sin() * 0.3;
            let sample = tone(60.0) + tone(1000.0) + tone(10_000.0);
            let (l, r) = unit.process(sample, sample);
            sum += l * l + r * r;
        }
        (sum / (PROBE_FRAMES * 2) as f32).sqrt()
    }

    fn unit_for(slot: &str) -> Box<dyn AudioUnit> {
        let descriptor = CLASSIC_3BAND.strip_slot(slot).expect("slot");
        make_unit(descriptor.unit_id, SAMPLE_RATE).expect("unit")
    }

    // Every other gain in the mixer smooths, and this one is automatable, so a drawn kill
    // lane would step a band to silence between two samples and click.
    #[test]
    fn an_isolator_band_reaches_its_new_gain_over_a_ramp_rather_than_at_once() {
        let mut unit = make_unit("isolator3band", SAMPLE_RATE).expect("the isolator");
        let tone =
            |frame: usize| (std::f32::consts::TAU * 60.0 * frame as f32 / SAMPLE_RATE).sin() * 0.3;
        for frame in 0..PROBE_FRAMES {
            unit.process(tone(frame), tone(frame));
        }

        unit.set_param("low", 0.0);
        let (stepped, _) = unit.process(tone(PROBE_FRAMES), tone(PROBE_FRAMES));
        let settled_at = PROBE_FRAMES + 4410;
        for frame in PROBE_FRAMES + 1..settled_at {
            unit.process(tone(frame), tone(frame));
        }
        let (settled, _) = unit.process(tone(settled_at), tone(settled_at));

        assert!(
            stepped.abs() > settled.abs() * 10.0,
            "the band vanished in one sample: stepped {stepped}, settled {settled}"
        );
    }

    // A param the manifest advertises but the unit ignores would be silently
    // inert: the editor would draw a lane that does nothing.
    #[test]
    fn every_eq_band_reaches_the_unit() {
        for band in ["low", "mid", "high"] {
            let flat = rms_through(unit_for("eq").as_mut());
            let mut cut = unit_for("eq");
            cut.set_param(band, -20.0);
            assert!(
                rms_through(cut.as_mut()) < flat * 0.95,
                "eq/{band} did not change the output"
            );
        }
    }

    #[test]
    fn the_filter_knob_and_its_switch_both_reach_the_unit() {
        let bypassed = rms_through(unit_for("filter").as_mut());

        // Sweeping with the filter off must stay inaudible, which is what makes
        // the second assertion about `active` meaningful rather than incidental.
        let mut swept_while_off = unit_for("filter");
        swept_while_off.set_param("value", -0.8);
        assert!((rms_through(swept_while_off.as_mut()) - bypassed).abs() < bypassed * 0.05);

        let mut swept = unit_for("filter");
        swept.set_param("value", -0.8);
        swept.set_param("active", 1.0);
        assert!(rms_through(swept.as_mut()) < bypassed * 0.95);
    }

    #[test]
    fn the_fader_scales_and_mute_is_not_reachable_as_a_param() {
        let unity = rms_through(unit_for("fader").as_mut());

        let mut quiet = unit_for("fader");
        quiet.set_param("gain", 0.25);
        assert!(rms_through(quiet.as_mut()) < unity * 0.5);

        let mut not_muted = unit_for("fader");
        not_muted.set_param("mute", 1.0);
        not_muted.set_param("muted", 1.0);
        assert!((rms_through(not_muted.as_mut()) - unity).abs() < unity * 0.01);

        let mut muted = unit_for("fader");
        muted.set_muted(true);
        // Measured after the mute has faded in, not across it: the 10 ms ramp
        // would otherwise dominate the window.
        rms_through(muted.as_mut());
        assert!(rms_through(muted.as_mut()) < unity * 0.01);
    }

    #[test]
    fn the_fader_curve_reaches_the_unit_without_moving_the_recorded_throw() {
        const HALF_THROW: f32 = 0.5;
        let at = |curve| {
            let mut fader = unit_for("fader");
            fader.set_param("gain", HALF_THROW);
            fader.set_fader_curve(curve);
            (rms_through(fader.as_mut()), fader.param("gain"))
        };
        let (linear, throw) = at(session_core::FaderCurve::Linear);
        let (exponential, _) = at(session_core::FaderCurve::Exponential);
        let (logarithmic, _) = at(session_core::FaderCurve::Logarithmic);

        assert!(exponential < linear);
        assert!(logarithmic > linear);
        assert_eq!(throw, Some(HALF_THROW));
    }

    #[test]
    fn the_manifest_addresses_the_fader_at_deck_scope() {
        assert!(CLASSIC_3BAND
            .descriptor(ParamScope::Deck, "fader", "gain")
            .is_some());
    }

    // A strip is used straight after construction, so a unit constructing itself away from
    // its descriptor default would start the session on a value the editor never shows.
    #[test]
    fn constructed_units_match_the_manifest_defaults() {
        for manifest in session_core::MANIFESTS {
            for slot in manifest.strip {
                let unit = make_unit(slot.unit_id, SAMPLE_RATE).expect("unit");
                for descriptor in slot.params {
                    assert_eq!(
                        unit.param(descriptor.id),
                        Some(descriptor.default as f32),
                        "{}: {}/{}",
                        manifest.id,
                        slot.slot,
                        descriptor.id
                    );
                }
            }
        }
    }

    // A mixer that colours the signal while doing nothing is a defect. The bands sum to an
    // allpass, so this checks magnitude: the phase shift is inaudible, a crossover notch is not.
    #[test]
    fn the_isolator_is_level_flat_at_unity() {
        for hz in [60.0, 300.0, 1000.0, 3000.0, 10_000.0] {
            let mut unit = make_unit("isolator3band", SAMPLE_RATE).expect("unit");
            let mut sum = 0.0;
            // Skips the first 1000 frames so the filters' startup transient is
            // not counted as level loss.
            for frame in 0..PROBE_FRAMES {
                let phase = frame as f32 / SAMPLE_RATE;
                let sample = (std::f32::consts::TAU * hz * phase).sin() * 0.5;
                let (l, _) = unit.process(sample, sample);
                if frame >= 1000 {
                    sum += l * l;
                }
            }
            let rms = (sum / (PROBE_FRAMES - 1000) as f32).sqrt();
            let expected = 0.5 / std::f32::consts::SQRT_2;
            assert!(
                (rms / expected - 1.0).abs() < 0.02,
                "{hz} Hz came through at {rms} instead of {expected}"
            );
        }
    }

    #[test]
    fn the_isolator_kills_a_band_outright() {
        for (band, hz) in [("low", 60.0), ("mid", 1000.0), ("high", 10_000.0)] {
            let mut unit = make_unit("isolator3band", SAMPLE_RATE).expect("unit");
            unit.set_param(band, 0.0);
            let tone = |frame: usize| {
                (std::f32::consts::TAU * hz * frame as f32 / SAMPLE_RATE).sin() * 0.5
            };
            // Past the gain ramp: the claim is that a killed band is silent, not that it
            // gets there in one sample.
            for frame in 0..PROBE_FRAMES {
                unit.process(tone(frame), tone(frame));
            }
            let mut sum = 0.0;
            for frame in PROBE_FRAMES..PROBE_FRAMES * 2 {
                let (l, _) = unit.process(tone(frame), tone(frame));
                sum += l * l;
            }
            let rms = (sum / PROBE_FRAMES as f32).sqrt();
            assert!(rms < 0.05, "{band} band survived its kill at rms {rms}");
        }
    }

    #[test]
    fn a_param_the_unit_does_not_have_reads_back_as_none() {
        let unit = make_unit("fader", SAMPLE_RATE).expect("unit");
        assert_eq!(unit.param("cutoff"), None);
    }
}
