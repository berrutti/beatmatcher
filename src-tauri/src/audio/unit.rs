use super::dsp::{
    EqRange, Equalizer, Equalizer4, Filter, FILTER_COEFF_REFRESH_INTERVAL, FILTER_RESONANCE_Q,
};
use session_core::{SlotDescriptor, Voicing};

const GAIN_SMOOTHING_TAU_SEC: f32 = 0.010;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StripSettings {
    pub fader_curve: session_core::FaderCurve,
    pub filter_resonance: f32,
}

impl Default for StripSettings {
    fn default() -> Self {
        Self {
            fader_curve: session_core::FaderCurve::default(),
            filter_resonance: FILTER_RESONANCE_Q,
        }
    }
}

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

    // Master scope, so it arrives outside the strip's own param space.
    fn apply_master(&mut self, _master: &StripSettings) {}

    fn send_gain(&self) -> f32 {
        0.0
    }

    fn is_sending(&self) -> bool {
        false
    }
}

struct SendTap {
    amount: f32,
    gain: Smoothed,
}

impl AudioUnit for SendTap {
    fn set_param(&mut self, param: &str, value: f32) {
        if param == "amount" {
            self.amount = value.clamp(0.0, 1.0);
            self.gain.set(self.amount);
        }
    }

    fn param(&self, param: &str) -> Option<f32> {
        (param == "amount").then_some(self.amount)
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.gain.advance();
        (l, r)
    }

    fn send_gain(&self) -> f32 {
        self.gain.current
    }

    fn is_sending(&self) -> bool {
        self.amount > 0.0
    }
}

struct SmoothedBands<const N: usize> {
    names: [&'static str; N],
    bands: [Smoothed; N],
    since_refresh: u32,
    stale: bool,
}

impl<const N: usize> SmoothedBands<N> {
    fn new(names: [&'static str; N], sample_rate: f32) -> Self {
        Self {
            names,
            bands: std::array::from_fn(|_| Smoothed::new(0.0, sample_rate, GAIN_SMOOTHING_TAU_SEC)),
            since_refresh: 0,
            stale: false,
        }
    }

    fn index(&self, param: &str) -> Option<usize> {
        self.names.iter().position(|name| *name == param)
    }

    fn set(&mut self, param: &str, db: f32) {
        if let Some(band) = self.index(param) {
            self.bands[band].set(db);
        }
    }

    fn target(&self, param: &str) -> Option<f32> {
        self.index(param).map(|band| self.bands[band].target)
    }

    fn advance(&mut self) -> Option<[f32; N]> {
        let before = self.bands.each_ref().map(|band| band.current);
        let now = self.bands.each_mut().map(Smoothed::advance);
        let moving = now != before;
        if moving {
            self.stale = true;
            self.since_refresh += 1;
        }
        if !self.stale || (moving && self.since_refresh < FILTER_COEFF_REFRESH_INTERVAL) {
            return None;
        }
        self.since_refresh = 0;
        self.stale = false;
        Some(now)
    }
}

struct Eq3Band {
    eq: Equalizer,
    bands: SmoothedBands<3>,
}

// The mid band answers to the high-mid role, not to "mid", so one controller mapping
// addresses this unit and the four-band one without being rewritten.
impl AudioUnit for Eq3Band {
    fn set_param(&mut self, param: &str, value: f32) {
        self.bands.set(param, value);
    }

    fn param(&self, param: &str) -> Option<f32> {
        self.bands.target(param)
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        if let Some([low, mid, high]) = self.bands.advance() {
            self.eq.set_low(low);
            self.eq.set_mid(mid);
            self.eq.set_high(high);
        }
        self.eq.process(l, r)
    }
}

struct Eq4Band {
    eq: Equalizer4,
    bands: SmoothedBands<4>,
}

impl AudioUnit for Eq4Band {
    fn set_param(&mut self, param: &str, value: f32) {
        self.bands.set(param, value);
    }

    fn param(&self, param: &str) -> Option<f32> {
        self.bands.target(param)
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        if let Some([low, low_mid, high_mid, high]) = self.bands.advance() {
            self.eq.set_low(low);
            self.eq.set_low_mid(low_mid);
            self.eq.set_high_mid(high_mid);
            self.eq.set_high(high);
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

    fn apply_master(&mut self, master: &StripSettings) {
        self.filter.set_resonance(master.filter_resonance);
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

    fn apply_master(&mut self, master: &StripSettings) {
        self.curve = master.fader_curve;
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

fn eq_range(slot: &SlotDescriptor, band: &str) -> Option<EqRange> {
    let band = slot.param(band)?;
    Some(EqRange {
        min_db: band.min as f32,
        max_db: band.max as f32,
    })
}

pub fn make_unit(slot: &SlotDescriptor, sample_rate: f32) -> Option<Box<dyn AudioUnit>> {
    match (slot.unit_id, &slot.voicing) {
        ("eq3band", Voicing::Eq3(voicing)) => Some(Box::new(Eq3Band {
            eq: Equalizer::new(sample_rate, voicing),
            bands: SmoothedBands::new(["low", "highMid", "high"], sample_rate),
        })),
        ("eq4band", Voicing::Eq4(voicing)) => Some(Box::new(Eq4Band {
            eq: Equalizer4::new(
                sample_rate,
                voicing,
                eq_range(slot, "low")?,
                eq_range(slot, "high")?,
            ),
            bands: SmoothedBands::new(["low", "lowMid", "highMid", "high"], sample_rate),
        })),
        ("sweep_filter", Voicing::None) => Some(Box::new(SweepFilter {
            filter: Filter::new(sample_rate),
            knob: 0.0,
            active: false,
        })),
        ("send_tap", Voicing::None) => Some(Box::new(SendTap {
            amount: 0.0,
            gain: Smoothed::new(0.0, sample_rate, GAIN_SMOOTHING_TAU_SEC),
        })),
        ("fader", Voicing::None) => Some(Box::new(Fader {
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
                    make_unit(slot, 44100.0).is_some(),
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
        make_unit(descriptor, SAMPLE_RATE).expect("unit")
    }

    #[test]
    fn every_eq_band_reaches_the_unit() {
        for band in ["low", "lowMid", "highMid", "high"] {
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
    fn the_resonance_starts_at_the_tuned_q() {
        for manifest in session_core::MANIFESTS {
            let descriptor = manifest
                .descriptor(ParamScope::Master, "filter", "resonance")
                .expect("a resonance control");
            assert_eq!(
                descriptor.default as f32,
                crate::audio::dsp::FILTER_RESONANCE_Q
            );
        }
    }

    #[test]
    fn the_master_resonance_reaches_the_filter() {
        let swept_at = |q: f32| {
            let mut filter = unit_for("filter");
            filter.set_param("value", -0.4);
            filter.set_param("active", 1.0);
            filter.apply_master(&StripSettings {
                filter_resonance: q,
                ..StripSettings::default()
            });
            rms_through(filter.as_mut())
        };
        let mild = swept_at(0.71);
        let wild = swept_at(5.0);
        assert!(
            (mild - wild).abs() > mild * 0.1,
            "resonance changed nothing: {mild} against {wild}"
        );
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
            fader.apply_master(&StripSettings {
                fader_curve: curve,
                ..StripSettings::default()
            });
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
                let unit = make_unit(slot, SAMPLE_RATE).expect("unit");
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
    fn a_strip_left_at_its_defaults_passes_the_signal_through_untouched() {
        for manifest in session_core::MANIFESTS {
            for slot in manifest.strip {
                let mut unit = make_unit(slot, SAMPLE_RATE).expect("unit");
                for frame in 0..SAMPLE_RATE as usize {
                    let phase = frame as f32 / SAMPLE_RATE;
                    let sample = (std::f32::consts::TAU * 440.0 * phase).sin() * 0.5;
                    unit.process(sample, sample);
                }
                for frame in 0..1024 {
                    let phase = (SAMPLE_RATE as usize + frame) as f32 / SAMPLE_RATE;
                    let sample = (std::f32::consts::TAU * 440.0 * phase).sin() * 0.5;
                    let (l, r) = unit.process(sample, sample);
                    assert!(
                        (l - sample).abs() < 1e-4 && (r - sample).abs() < 1e-4,
                        "{} slot '{}' altered a resting signal: {sample} became {l}",
                        manifest.id,
                        slot.slot
                    );
                }
            }
        }
    }

    #[test]
    fn an_eq_voiced_for_the_other_unit_is_refused() {
        let four_band = CLASSIC_4BAND.strip_slot("eq").expect("eq slot");
        let three_band = session_core::CLASSIC_3BAND
            .strip_slot("eq")
            .expect("eq slot");
        let crossed = SlotDescriptor {
            slot: four_band.slot,
            unit_id: four_band.unit_id,
            voicing: match three_band.voicing {
                Voicing::Eq3(voicing) => Voicing::Eq3(voicing),
                _ => panic!("the three band is voiced as one"),
            },
            params: four_band.params,
        };
        assert!(make_unit(&crossed, SAMPLE_RATE).is_none());
    }

    #[test]
    fn an_eq_band_reads_back_the_value_set_while_still_ramping_to_it() {
        for manifest in session_core::MANIFESTS {
            let slot = manifest.strip_slot("eq").expect("eq slot");
            let mut unit = make_unit(slot, SAMPLE_RATE).expect("unit");
            for band in slot.params {
                unit.set_param(band.id, -12.0);
            }
            unit.process(0.5, 0.5);
            for band in slot.params {
                assert_eq!(unit.param(band.id), Some(-12.0), "{}", manifest.id);
            }
        }
    }

    #[test]
    fn a_settled_eq_is_designed_at_its_target() {
        let mut bands = SmoothedBands::new(["low"], SAMPLE_RATE);
        bands.set("low", -12.0);
        let mut designed = None;
        for _ in 0..SAMPLE_RATE as usize {
            designed = bands.advance().or(designed);
        }
        assert_eq!(designed, Some([-12.0]));
    }

    #[test]
    fn a_param_the_unit_does_not_have_reads_back_as_none() {
        let unit = make_unit(
            CLASSIC_4BAND.strip_slot("fader").expect("fader slot"),
            SAMPLE_RATE,
        )
        .expect("unit");
        assert_eq!(unit.param("cutoff"), None);
    }
}
