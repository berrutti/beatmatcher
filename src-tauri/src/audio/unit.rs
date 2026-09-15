use super::dsp::{Equalizer, Equalizer4, Filter, FILTER_COEFF_REFRESH_INTERVAL};

const GAIN_SMOOTHING_TAU_SEC: f32 = 0.010;

struct Smoothed {
    current: f32,
    target: f32,
    coeff: f32,
}

impl Smoothed {
    fn new(value: f32, sample_rate: f32, tau_sec: f32) -> Self {
        Self {
            current: value,
            target: value,
            coeff: 1.0 - (-1.0 / (sample_rate * tau_sec)).exp(),
        }
    }

    fn set(&mut self, target: f32) {
        self.target = target;
    }

    fn advance(&mut self) -> f32 {
        self.current = approach(self.current, self.target, self.coeff);
        self.current
    }
}

pub trait AudioUnit: Send {
    fn set_param(&mut self, param: &str, value: f32);

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
    eq: Equalizer,
    low: Smoothed,
    mid: Smoothed,
    high: Smoothed,
    since_refresh: u32,
    stale: bool,
}

// The mid band answers to the high-mid role, not to "mid", so one controller mapping
// addresses this unit and the four-band one without being rewritten.
impl AudioUnit for Eq3Band {
    fn set_param(&mut self, param: &str, value: f32) {
        match param {
            "low" => self.low.set(value),
            "highMid" => self.mid.set(value),
            "high" => self.high.set(value),
            _ => {}
        }
    }

    // The target, not the smoothed value: a readback mid-ramp must report the
    // knob the user set rather than where the ramp happens to be.
    fn param(&self, param: &str) -> Option<f32> {
        match param {
            "low" => Some(self.low.target),
            "highMid" => Some(self.mid.target),
            "high" => Some(self.high.target),
            _ => None,
        }
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let before = (self.low.current, self.mid.current, self.high.current);
        let now = (self.low.advance(), self.mid.advance(), self.high.advance());
        if now != before {
            self.stale = true;
            self.since_refresh += 1;
        }
        // Designing three biquads per sample is what the divider avoids; the
        // `now == before` arm catches the last step so a settled EQ is exact.
        if self.stale && (self.since_refresh >= FILTER_COEFF_REFRESH_INTERVAL || now == before) {
            self.eq.set_low(now.0);
            self.eq.set_mid(now.1);
            self.eq.set_high(now.2);
            self.since_refresh = 0;
            self.stale = false;
        }
        self.eq.process(l, r)
    }
}

struct Eq4Band {
    eq: Equalizer4,
    low: Smoothed,
    low_mid: Smoothed,
    high_mid: Smoothed,
    high: Smoothed,
    since_refresh: u32,
    stale: bool,
}

impl AudioUnit for Eq4Band {
    fn set_param(&mut self, param: &str, value: f32) {
        match param {
            "low" => self.low.set(value),
            "lowMid" => self.low_mid.set(value),
            "highMid" => self.high_mid.set(value),
            "high" => self.high.set(value),
            _ => {}
        }
    }

    // The target, not the smoothed value: a readback mid-ramp must report the
    // knob the user set rather than where the ramp happens to be.
    fn param(&self, param: &str) -> Option<f32> {
        match param {
            "low" => Some(self.low.target),
            "lowMid" => Some(self.low_mid.target),
            "highMid" => Some(self.high_mid.target),
            "high" => Some(self.high.target),
            _ => None,
        }
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let before = (
            self.low.current,
            self.low_mid.current,
            self.high_mid.current,
            self.high.current,
        );
        let now = (
            self.low.advance(),
            self.low_mid.advance(),
            self.high_mid.advance(),
            self.high.advance(),
        );
        if now != before {
            self.stale = true;
            self.since_refresh += 1;
        }
        // Designing four biquads per sample is what the divider avoids; the
        // `now == before` arm catches the last step so a settled EQ is exact.
        if self.stale && (self.since_refresh >= FILTER_COEFF_REFRESH_INTERVAL || now == before) {
            self.eq.set_low(now.0);
            self.eq.set_low_mid(now.1);
            self.eq.set_high_mid(now.2);
            self.eq.set_high(now.3);
            self.since_refresh = 0;
            self.stale = false;
        }
        self.eq.process(l, r)
    }
}

struct SweepFilter {
    filter: Filter,
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
    gain: Smoothed,
    muted: bool,
    mute: Smoothed,
}

impl Fader {
    // Curved before smoothing, so the one-pole interpolates what is heard.
    fn resolve_gain(&mut self) {
        self.gain
            .set(self.curve.gain(f64::from(self.position)) as f32);
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
        // Mute fades over the same time constant as the fader to avoid clicks.
        self.mute.set(if self.muted { 0.0 } else { 1.0 });
        let gain = self.gain.advance() * self.mute.advance();
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

pub fn make_unit(unit_id: &str, sample_rate: f32) -> Option<Box<dyn AudioUnit>> {
    match unit_id {
        "eq3band" => Some(Box::new(Eq3Band {
            eq: Equalizer::new(sample_rate),
            low: Smoothed::new(0.0, sample_rate, GAIN_SMOOTHING_TAU_SEC),
            mid: Smoothed::new(0.0, sample_rate, GAIN_SMOOTHING_TAU_SEC),
            high: Smoothed::new(0.0, sample_rate, GAIN_SMOOTHING_TAU_SEC),
            since_refresh: 0,
            stale: false,
        })),
        "eq4band" => Some(Box::new(Eq4Band {
            eq: Equalizer4::new(sample_rate),
            low: Smoothed::new(0.5, sample_rate, GAIN_SMOOTHING_TAU_SEC),
            low_mid: Smoothed::new(0.0, sample_rate, GAIN_SMOOTHING_TAU_SEC),
            high_mid: Smoothed::new(0.0, sample_rate, GAIN_SMOOTHING_TAU_SEC),
            high: Smoothed::new(0.5, sample_rate, GAIN_SMOOTHING_TAU_SEC),
            since_refresh: 0,
            stale: false,
        })),
        "sweep_filter" => Some(Box::new(SweepFilter {
            filter: Filter::new(sample_rate),
            knob: 0.0,
            active: false,
        })),
        "fader" => Some(Box::new(Fader {
            position: 1.0,
            curve: session_core::FaderCurve::default(),
            gain: Smoothed::new(1.0, sample_rate, GAIN_SMOOTHING_TAU_SEC),
            muted: false,
            mute: Smoothed::new(1.0, sample_rate, GAIN_SMOOTHING_TAU_SEC),
        })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use session_core::{ParamScope, CLASSIC_4BAND};

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
        let descriptor = CLASSIC_4BAND.strip_slot(slot).expect("slot");
        make_unit(descriptor.unit_id, SAMPLE_RATE).expect("unit")
    }

    #[test]
    fn every_eq_band_reaches_the_unit() {
        // The shelves travel 0..1 from a 0.5 detent; the bells are dB about zero.
        for (band, cut_to) in [
            ("low", 0.0),
            ("lowMid", -20.0),
            ("highMid", -20.0),
            ("high", 0.0),
        ] {
            let flat = rms_through(unit_for("eq").as_mut());
            let mut cut = unit_for("eq");
            cut.set_param(band, cut_to);
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
        assert!(CLASSIC_4BAND
            .descriptor(ParamScope::Deck, "fader", "gain")
            .is_some());
    }

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

    #[test]
    fn a_param_the_unit_does_not_have_reads_back_as_none() {
        let unit = make_unit("fader", SAMPLE_RATE).expect("unit");
        assert_eq!(unit.param("cutoff"), None);
    }
}
