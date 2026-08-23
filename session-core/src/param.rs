// `slot` is the position in the strip, not the unit filling it, so swapping a
// unit keeps existing automation pointing at the same place.

pub enum ParamUnit {
    Db,
    Normalized,
    Bool,
    Ratio,
}

impl ParamUnit {
    pub fn id(&self) -> &'static str {
        match self {
            Self::Db => "db",
            Self::Normalized => "normalized",
            Self::Bool => "bool",
            Self::Ratio => "ratio",
        }
    }
}

pub enum Taper {
    Linear,
    Bipolar { center: f64 },
}

pub struct ParamDescriptor {
    pub id: &'static str,
    pub label: &'static str,
    pub short_label: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub step: f64,
    pub unit: ParamUnit,
    pub taper: Taper,
    pub dead_zone: Option<f64>,
    pub automatable: bool,
    pub lane_group: u8,
}

impl ParamDescriptor {
    /// Places a normalized control position on this param's range, quantized to `step`.
    /// A bipolar param splits the travel at its centre, so a detent reads unity.
    pub fn from_unit_interval(&self, position: f64) -> f64 {
        let position = position.clamp(0.0, 1.0);
        let raw = match self.taper {
            Taper::Linear => self.min + position * (self.max - self.min),
            Taper::Bipolar { center } if position < 0.5 => {
                self.min + (position / 0.5) * (center - self.min)
            }
            Taper::Bipolar { center } => center + ((position - 0.5) / 0.5) * (self.max - center),
        };
        if self.step <= 0.0 {
            return raw;
        }
        (self.min + ((raw - self.min) / self.step).round() * self.step).clamp(self.min, self.max)
    }

    /// A `.bms` event and a MIDI mapping both reach a param directly, and a unit is free
    /// to trust what it is handed, so the range is enforced once for all of them.
    pub fn clamp(&self, value: f64) -> f64 {
        value.clamp(self.min, self.max)
    }
}

impl ParamDescriptor {
    /// Everything that changes what a recorded value means. Labels and lane
    /// grouping are excluded for the same reason `content_hash` excludes them.
    fn same_semantics(&self, other: &ParamDescriptor) -> bool {
        self.min == other.min
            && self.max == other.max
            && self.default == other.default
            && self.step == other.step
            && self.dead_zone == other.dead_zone
            && self.automatable == other.automatable
    }
}

pub struct SlotDescriptor {
    pub slot: &'static str,
    pub unit_id: &'static str,
    pub params: &'static [ParamDescriptor],
}

impl SlotDescriptor {
    pub fn param(&self, id: &str) -> Option<&ParamDescriptor> {
        self.params.iter().find(|param| param.id == id)
    }
}

pub struct MixerManifest {
    pub id: &'static str,
    pub strip: &'static [SlotDescriptor],
    pub master: &'static [SlotDescriptor],
    // Cue is everything up to but excluding this slot.
    pub cue_tap: &'static str,
}

// The cue sheet decides a track is audible from the fader, not from the mix, so
// every manifest has to provide this address whatever unit fills the slot.
pub const FADER_GAIN: (&str, &str) = ("fader", "gain");

pub const REQUIRED_STRIP_ROLES: &[(&str, &str)] = &[FADER_GAIN];

pub fn is_fader_gain(slot: &str, param: &str) -> bool {
    (slot, param) == FADER_GAIN
}

impl MixerManifest {
    pub fn strip_slot(&self, slot: &str) -> Option<&SlotDescriptor> {
        self.strip.iter().find(|entry| entry.slot == slot)
    }

    pub fn master_slot(&self, slot: &str) -> Option<&SlotDescriptor> {
        self.master.iter().find(|entry| entry.slot == slot)
    }

    pub fn descriptor(
        &self,
        scope: ParamScope,
        slot: &str,
        param: &str,
    ) -> Option<&ParamDescriptor> {
        match scope {
            ParamScope::Deck => self.strip_slot(slot)?.param(param),
            ParamScope::Master => self.master_slot(slot)?.param(param),
        }
    }

    /// Whether this manifest can replay everything `other` can contain, with identical
    /// meaning. A manifest that only adds slots hosts its predecessor.
    pub fn can_host(&self, other: &MixerManifest) -> bool {
        if self.cue_tap != other.cue_tap {
            return false;
        }
        let scopes = [
            (ParamScope::Deck, other.strip, self.strip),
            (ParamScope::Master, other.master, self.master),
        ];
        for (scope, theirs, mine_slots) in scopes {
            // The strip is a signal chain and cue taps a point along it, so the same
            // units in another order are a different mix. Master slots run in parallel.
            let chained = scope == ParamScope::Deck;
            let mut previous: Option<usize> = None;
            for slot in theirs {
                // The unit decides how a value sounds, so the same range filled by a
                // different unit is not the same address.
                let Some(position) = mine_slots
                    .iter()
                    .position(|entry| entry.slot == slot.slot && entry.unit_id == slot.unit_id)
                else {
                    return false;
                };
                if chained && previous.is_some_and(|earlier| position <= earlier) {
                    return false;
                }
                previous = Some(position);
                for param in slot.params {
                    let Some(mine) = self.descriptor(scope, slot.slot, param.id) else {
                        return false;
                    };
                    if !mine.same_semantics(param) {
                        return false;
                    }
                }
            }
        }
        true
    }

    pub fn validate(&self) -> Result<(), String> {
        for (slot, param) in REQUIRED_STRIP_ROLES {
            self.strip_slot(slot)
                .and_then(|entry| entry.param(param))
                .ok_or_else(|| format!("manifest '{}' is missing {slot}/{param}", self.id))?;
        }
        if self.strip_slot(self.cue_tap).is_none() {
            return Err(format!(
                "manifest '{}' taps cue at '{}', which is not a strip slot",
                self.id, self.cue_tap
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamScope {
    Deck,
    Master,
}

pub static MANIFESTS: &[&MixerManifest] = &[
    &CLASSIC_3BAND,
    &ISOLATOR_3BAND,
    &CLASSIC_3BAND_V2,
    &ISOLATOR_3BAND_V2,
];

pub fn manifest_by_id(id: &str) -> Option<&'static MixerManifest> {
    MANIFESTS.iter().copied().find(|manifest| manifest.id == id)
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct MixerHeader {
    pub id: String,
    pub hash: String,
}

/// The manifest a session must be replayed with. A session with no header
/// predates manifests, and every one of those was played on the classic mixer.
pub fn resolve_manifest(header: Option<&MixerHeader>) -> Result<&'static MixerManifest, String> {
    let Some(header) = header else {
        return Ok(&CLASSIC_3BAND);
    };
    let manifest = manifest_by_id(&header.id).ok_or_else(|| {
        format!(
            "session needs mixer '{}', which this build does not have",
            header.id
        )
    })?;
    let hash = manifest.content_hash();
    if hash != header.hash {
        return Err(format!(
            "mixer '{}' has changed since this session was recorded ({} now, {} then)",
            header.id, hash, header.hash
        ));
    }
    Ok(manifest)
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_bytes(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |acc, byte| {
        (acc ^ *byte as u64).wrapping_mul(FNV_PRIME)
    })
}

impl MixerManifest {
    /// Identifies the manifest by everything that changes what a `set_param` means.
    /// Labels and lane grouping are excluded, so renaming a knob keeps sessions valid.
    pub fn content_hash(&self) -> String {
        let mut hash = fnv_bytes(FNV_OFFSET, self.id.as_bytes());
        hash = fnv_bytes(hash, self.cue_tap.as_bytes());
        for (scope, slots) in [("deck", self.strip), ("master", self.master)] {
            hash = fnv_bytes(hash, scope.as_bytes());
            for slot in slots {
                hash = fnv_bytes(hash, slot.slot.as_bytes());
                hash = fnv_bytes(hash, slot.unit_id.as_bytes());
                for param in slot.params {
                    hash = fnv_bytes(hash, param.id.as_bytes());
                    for value in [param.min, param.max, param.default, param.step] {
                        hash = fnv_bytes(hash, &value.to_bits().to_le_bytes());
                    }
                    hash = fnv_bytes(
                        hash,
                        &param.dead_zone.unwrap_or(f64::NAN).to_bits().to_le_bytes(),
                    );
                    hash = fnv_bytes(hash, &[param.automatable as u8]);
                }
            }
        }
        format!("{hash:016x}")
    }

    pub fn header(&self) -> MixerHeader {
        MixerHeader {
            id: self.id.to_string(),
            hash: self.content_hash(),
        }
    }
}

// A static rather than a const so callers can hold `&'static ParamDescriptor`
// instead of borrowing a per-use temporary.
pub static CLASSIC_3BAND: MixerManifest = MixerManifest {
    id: "classic-3band",
    cue_tap: "fader",
    strip: CLASSIC_STRIP,
    master: MASTER_SLOTS,
};

const CLASSIC_STRIP: &[SlotDescriptor] = &[
    SlotDescriptor {
        slot: "eq",
        unit_id: "eq3band",
        params: &[
            eq_param("low", "Low", "LO"),
            eq_param("mid", "Mid", "MD"),
            eq_param("high", "High", "HI"),
        ],
    },
    SWEEP_FILTER_SLOT,
    FADER_SLOT,
];

const SWEEP_FILTER_SLOT: SlotDescriptor = SlotDescriptor {
    slot: "filter",
    unit_id: "sweep_filter",
    params: &[
        ParamDescriptor {
            id: "value",
            label: "Filter",
            short_label: "F",
            min: -1.0,
            max: 1.0,
            default: 0.0,
            step: 0.01,
            unit: ParamUnit::Normalized,
            taper: Taper::Bipolar { center: 0.0 },
            dead_zone: Some(crate::FILTER_DEAD_ZONE),
            automatable: true,
            lane_group: 1,
        },
        ParamDescriptor {
            id: "active",
            label: "Filter on",
            short_label: "FA",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 1.0,
            unit: ParamUnit::Bool,
            taper: Taper::Linear,
            dead_zone: None,
            automatable: false,
            lane_group: 1,
        },
    ],
};

const FADER_SLOT: SlotDescriptor = SlotDescriptor {
    slot: "fader",
    unit_id: "fader",
    params: &[ParamDescriptor {
        id: "gain",
        label: "Volume",
        short_label: "G",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        step: 0.01,
        unit: ParamUnit::Normalized,
        taper: Taper::Linear,
        dead_zone: None,
        automatable: true,
        lane_group: 0,
    }],
};

const MASTER_GAIN_SLOT: SlotDescriptor = SlotDescriptor {
    slot: "gain",
    unit_id: "master_gain",
    params: &[ParamDescriptor {
        id: "gain",
        label: "Master",
        short_label: "M",
        min: 0.0,
        max: 1.0,
        default: crate::DEFAULT_MASTER_GAIN as f64,
        step: 0.01,
        unit: ParamUnit::Normalized,
        taper: Taper::Linear,
        dead_zone: None,
        automatable: true,
        lane_group: 0,
    }],
};

// Symmetric about zero, so no bipolar taper is needed to make the centre detent
// read as centre. Pinned by `a_bipolar_taper_over_a_symmetric_range_stays_linear`.
const XFADER_SLOT: SlotDescriptor = SlotDescriptor {
    slot: "xfader",
    unit_id: "xfader",
    params: &[ParamDescriptor {
        id: "position",
        label: "Crossfader",
        short_label: "X",
        min: -1.0,
        max: 1.0,
        default: 0.0,
        step: 0.01,
        unit: ParamUnit::Normalized,
        taper: Taper::Linear,
        dead_zone: None,
        automatable: true,
        lane_group: 0,
    }],
};

/// The crossfader's own descriptor, reachable even from a manifest whose master has
/// no crossfader slot, so the editor can draw the lane for every session.
pub fn xfader_position_descriptor() -> &'static ParamDescriptor {
    &XFADER_SLOT.params[0]
}

/// Which crossfader bus a strip is multiplied by. `Thru` is the default so a
/// session that predates the crossfader, or never assigns one, is unaffected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum XfaderAssign {
    #[default]
    Thru,
    A,
    B,
}

impl XfaderAssign {
    pub fn as_str(&self) -> &'static str {
        match self {
            XfaderAssign::Thru => "thru",
            XfaderAssign::A => "a",
            XfaderAssign::B => "b",
        }
    }

    /// Infallible so that a session written by a newer build degrades to an inert
    /// crossfader rather than failing to load.
    pub fn from_str_or_thru(value: &str) -> Self {
        match value {
            "a" => XfaderAssign::A,
            "b" => XfaderAssign::B,
            _ => XfaderAssign::Thru,
        }
    }

    pub fn gain(&self, position: f64) -> f64 {
        let (a, b) = xfader_gains(position);
        match self {
            XfaderAssign::Thru => 1.0,
            XfaderAssign::A => a,
            XfaderAssign::B => b,
        }
    }
}

/// Constant power, with both buses at -3 dB when centred, so a blend holds its
/// perceived level instead of dipping through the middle.
pub fn xfader_gains(position: f64) -> (f64, f64) {
    // The ends are returned exactly, because `cos` at a quarter turn is 6e-17 and anything
    // asking whether a deck is silent would read that as audible.
    if position <= -1.0 {
        return (1.0, 0.0);
    }
    if position >= 1.0 {
        return (0.0, 1.0);
    }
    let angle = (position + 1.0) / 2.0 * std::f64::consts::FRAC_PI_2;
    (angle.cos(), angle.sin())
}

/// How a channel fader's throw maps to gain. Categorical, so it travels by name
/// and gets its own event, the way `XfaderAssign` does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FaderCurve {
    Exponential,
    /// The only curve that existed before the param did, so it is the default a
    /// session that never sets one resolves to.
    #[default]
    Linear,
    Logarithmic,
}

/// Puts half throw at -12 dB on the exponential curve, which is about where a
/// hardware channel fader sits.
const FADER_CURVE_EXPONENT: f64 = 2.0;

impl FaderCurve {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exponential => "exponential",
            Self::Linear => "linear",
            Self::Logarithmic => "logarithmic",
        }
    }

    /// Infallible so a session naming a curve this build does not have plays on a
    /// linear fader rather than failing to load.
    pub fn from_str_or_linear(value: &str) -> Self {
        match value {
            "exponential" => Self::Exponential,
            "logarithmic" => Self::Logarithmic,
            _ => Self::Linear,
        }
    }

    pub fn gain(self, position: f64) -> f64 {
        let position = position.clamp(0.0, 1.0);
        match self {
            Self::Exponential => position.powf(FADER_CURVE_EXPONENT),
            Self::Linear => position,
            Self::Logarithmic => position.powf(1.0 / FADER_CURVE_EXPONENT),
        }
    }
}

/// How much audio one jog tick covers, as the rpm the platter is standing in for.
/// Categorical, so it travels by name and gets its own event, the way `FaderCurve` does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JogRotationSpeed {
    #[default]
    Rpm33,
    Rpm45,
}

const RPM_33: f64 = 33.0 + 1.0 / 3.0;
const RPM_45: f64 = 45.0;

/// A revolution covers 60/rpm seconds of audio, so the scale is the ratio of the two.
pub const JOG_SCRUB_SEC_PER_TICK_AT_33: f64 = 0.002;

/// A paused platter scrubs this many times further than a playing one bends.
pub const JOG_PAUSED_MULTIPLIER: f64 = 100.0;

pub const JOG_SHIFT_MULTIPLIER: f64 = 2.0;

/// The wheel filter settles over this, which is what spreads one gesture's travel out
/// in time. The total is unaffected, so only a reader tracking the settle needs it.
pub const JOG_FILTER_TAU_SEC: f64 = 0.040;

/// How much of a gesture's travel has arrived `elapsed_sec` after the wheel moved.
pub fn jog_settled_fraction(elapsed_sec: f64) -> f64 {
    1.0 - (-elapsed_sec / JOG_FILTER_TAU_SEC).exp()
}

impl JogRotationSpeed {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rpm33 => "rpm33",
            Self::Rpm45 => "rpm45",
        }
    }

    /// Infallible so a session naming a speed this build does not have scrubs at 33
    /// rather than failing to load.
    pub fn from_str_or_33(value: &str) -> Self {
        match value {
            "rpm45" => Self::Rpm45,
            _ => Self::Rpm33,
        }
    }

    /// Against 33, which is the speed `JOG_SCRUB_SEC_PER_TICK_AT_33` is set for.
    pub fn scrub_scale(self) -> f64 {
        match self {
            Self::Rpm33 => 1.0,
            Self::Rpm45 => RPM_33 / RPM_45,
        }
    }

    /// What one logged tick is worth. The engine's filter shapes when this travel
    /// happens and never how much, so the total is reproducible without it.
    pub fn frames_per_tick(self, sample_rate: f64) -> f64 {
        self.sec_per_tick() * sample_rate
    }

    /// The sample rate cancels out of `frames_per_tick / sample_rate`, so a reader
    /// working in track seconds needs no device to ask what a tick was worth.
    pub fn sec_per_tick(self) -> f64 {
        JOG_SCRUB_SEC_PER_TICK_AT_33 * self.scrub_scale()
    }
}

const MASTER_SLOTS: &[SlotDescriptor] = &[MASTER_GAIN_SLOT];

const MASTER_SLOTS_V2: &[SlotDescriptor] = &[MASTER_GAIN_SLOT, XFADER_SLOT];

// The classic strip with a different unit in the `eq` slot: full kill over a shelf, so
// the param ids match but the ranges do not. The hash keeps the two apart.
pub static ISOLATOR_3BAND: MixerManifest = MixerManifest {
    id: "isolator-3band",
    cue_tap: "fader",
    strip: ISOLATOR_STRIP,
    master: MASTER_SLOTS,
};

const ISOLATOR_STRIP: &[SlotDescriptor] = &[
    SlotDescriptor {
        slot: "eq",
        unit_id: "isolator3band",
        params: &[
            isolator_param("low", "Low", "LO"),
            isolator_param("mid", "Mid", "MD"),
            isolator_param("high", "High", "HI"),
        ],
    },
    SWEEP_FILTER_SLOT,
    FADER_SLOT,
];

// The v1 manifests stay frozen so pre-crossfader sessions still resolve one by id.
// Adding the slot in place would have changed their hash and refused every file.
pub static CLASSIC_3BAND_V2: MixerManifest = MixerManifest {
    id: "classic-3band-v2",
    cue_tap: "fader",
    strip: CLASSIC_STRIP,
    master: MASTER_SLOTS_V2,
};

pub static ISOLATOR_3BAND_V2: MixerManifest = MixerManifest {
    id: "isolator-3band-v2",
    cue_tap: "fader",
    strip: ISOLATOR_STRIP,
    master: MASTER_SLOTS_V2,
};

const fn isolator_param(
    id: &'static str,
    label: &'static str,
    short_label: &'static str,
) -> ParamDescriptor {
    ParamDescriptor {
        id,
        label,
        short_label,
        min: 0.0,
        max: 1.0,
        default: 1.0,
        step: 0.01,
        unit: ParamUnit::Normalized,
        taper: Taper::Linear,
        dead_zone: None,
        automatable: true,
        lane_group: 3,
    }
}

const fn eq_param(
    id: &'static str,
    label: &'static str,
    short_label: &'static str,
) -> ParamDescriptor {
    ParamDescriptor {
        id,
        label,
        short_label,
        min: crate::EQ_MIN_DB,
        max: crate::EQ_MAX_DB,
        default: 0.0,
        step: 0.5,
        unit: ParamUnit::Db,
        // Centre-detented on hardware, so the detent has to read unity even
        // though the kill side is far longer than the boost side.
        taper: Taper::Bipolar { center: 0.0 },
        dead_zone: None,
        automatable: true,
        lane_group: 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lane_edit::{lane_spec_for, EditableLane};

    const fn fader_gain_param(
        label: &'static str,
        short_label: &'static str,
        max: f64,
    ) -> ParamDescriptor {
        ParamDescriptor {
            id: "gain",
            label,
            short_label,
            min: 0.0,
            max,
            default: 1.0,
            step: 0.01,
            unit: ParamUnit::Normalized,
            taper: Taper::Linear,
            dead_zone: None,
            automatable: true,
            lane_group: 0,
        }
    }

    #[test]
    fn every_fader_curve_holds_the_ends_of_the_throw() {
        for curve in [
            FaderCurve::Exponential,
            FaderCurve::Linear,
            FaderCurve::Logarithmic,
        ] {
            assert_eq!(curve.gain(0.0), 0.0);
            assert_eq!(curve.gain(1.0), 1.0);
        }
    }

    #[test]
    fn the_fader_curves_order_from_quietest_to_loudest_across_the_throw() {
        for step in 1..10 {
            let position = f64::from(step) / 10.0;
            assert!(FaderCurve::Exponential.gain(position) < FaderCurve::Linear.gain(position));
            assert!(FaderCurve::Linear.gain(position) < FaderCurve::Logarithmic.gain(position));
        }
    }

    #[test]
    fn every_fader_curve_round_trips_through_the_name_it_reports() {
        for curve in [
            FaderCurve::Exponential,
            FaderCurve::Linear,
            FaderCurve::Logarithmic,
        ] {
            let name = curve.as_str();
            assert_eq!(FaderCurve::from_str_or_linear(name), curve);
            assert_eq!(
                serde_json::from_str::<FaderCurve>(&format!("\"{name}\"")).expect("a curve name"),
                curve
            );
        }
    }

    #[test]
    fn the_fader_curve_defaults_to_linear() {
        assert_eq!(FaderCurve::default(), FaderCurve::Linear);
        assert_eq!(
            FaderCurve::from_str_or_linear("sawtooth"),
            FaderCurve::Linear
        );
    }

    #[test]
    fn the_crossfader_ends_hold_one_bus_open_and_close_the_other() {
        assert_eq!(xfader_gains(-1.0), (1.0, 0.0));
        assert_eq!(xfader_gains(1.0), (0.0, 1.0));
        assert_eq!(XfaderAssign::A.gain(1.0), 0.0);
        assert_eq!(XfaderAssign::B.gain(-1.0), 0.0);
    }

    #[test]
    fn the_crossfader_holds_power_across_its_throw() {
        for step in 0..=20 {
            let position = -1.0 + f64::from(step) / 10.0;
            let (a, b) = xfader_gains(position);
            assert!(
                (a * a + b * b - 1.0).abs() < 1e-9,
                "power dipped at {position}"
            );
        }
    }

    #[test]
    fn a_position_beyond_the_ends_is_clamped() {
        assert_eq!(xfader_gains(-4.0), xfader_gains(-1.0));
        assert_eq!(xfader_gains(4.0), xfader_gains(1.0));
    }

    #[test]
    fn a_thru_strip_ignores_the_crossfader() {
        for step in 0..=20 {
            let position = -1.0 + f64::from(step) / 10.0;
            assert_eq!(XfaderAssign::Thru.gain(position), 1.0);
        }
    }

    #[test]
    fn an_assigned_strip_follows_its_own_bus() {
        assert_eq!(XfaderAssign::A.gain(-1.0), 1.0);
        assert_eq!(XfaderAssign::A.gain(1.0), 0.0);
        assert_eq!(XfaderAssign::B.gain(-1.0), 0.0);
        assert_eq!(XfaderAssign::B.gain(1.0), 1.0);
    }

    #[test]
    fn an_assign_round_trips_and_an_unknown_one_reads_as_thru() {
        for assign in [XfaderAssign::Thru, XfaderAssign::A, XfaderAssign::B] {
            assert_eq!(XfaderAssign::from_str_or_thru(assign.as_str()), assign);
        }
        assert_eq!(XfaderAssign::from_str_or_thru("c"), XfaderAssign::Thru);
        assert_eq!(XfaderAssign::from_str_or_thru(""), XfaderAssign::Thru);
    }

    #[test]
    fn versioning_the_mixer_left_the_frozen_manifests_alone() {
        assert!(CLASSIC_3BAND
            .descriptor(ParamScope::Master, "xfader", "position")
            .is_none());
        assert!(ISOLATOR_3BAND
            .descriptor(ParamScope::Master, "xfader", "position")
            .is_none());
        assert!(CLASSIC_3BAND_V2
            .descriptor(ParamScope::Master, "xfader", "position")
            .is_some());
        assert!(ISOLATOR_3BAND_V2
            .descriptor(ParamScope::Master, "xfader", "position")
            .is_some());
        assert_ne!(
            CLASSIC_3BAND.content_hash(),
            CLASSIC_3BAND_V2.content_hash()
        );
    }

    #[test]
    fn every_shipped_manifest_keeps_the_hash_its_sessions_carry() {
        let pinned: &[(&str, &str)] = &[
            ("classic-3band", "7c5d3cab7af37be4"),
            ("isolator-3band", "7506fbbc0d1160a9"),
            ("classic-3band-v2", "a185b94836f236e4"),
            ("isolator-3band-v2", "93c120a76ac1a289"),
        ];
        let shipped: Vec<&str> = MANIFESTS.iter().map(|manifest| manifest.id).collect();
        let listed: Vec<&str> = pinned.iter().map(|(id, _)| *id).collect();
        assert_eq!(shipped, listed);
        for (id, hash) in pinned {
            let manifest = manifest_by_id(id).unwrap_or_else(|| panic!("{id}"));
            assert_eq!(&manifest.content_hash(), hash, "{id}");
        }
    }

    #[test]
    fn the_shipped_manifest_is_valid() {
        CLASSIC_3BAND.validate().expect("classic-3band");
    }

    #[test]
    fn a_manifest_without_the_fader_role_is_rejected() {
        const NO_FADER: MixerManifest = MixerManifest {
            id: "broken",
            cue_tap: "eq",
            strip: &[SlotDescriptor {
                slot: "eq",
                unit_id: "eq3band",
                params: &[eq_param("low", "Low", "LO")],
            }],
            master: &[],
        };
        assert!(NO_FADER.validate().is_err());
    }

    #[test]
    fn a_manifest_tapping_cue_at_an_unknown_slot_is_rejected() {
        const BAD_TAP: MixerManifest = MixerManifest {
            id: "broken",
            cue_tap: "nope",
            strip: &[SlotDescriptor {
                slot: "fader",
                unit_id: "fader",
                params: &[ParamDescriptor {
                    id: "gain",
                    label: "Volume",
                    short_label: "G",
                    min: 0.0,
                    max: 1.0,
                    default: 1.0,
                    step: 0.01,
                    unit: ParamUnit::Normalized,
                    taper: Taper::Linear,
                    dead_zone: None,
                    automatable: true,
                    lane_group: 0,
                }],
            }],
            master: &[],
        };
        assert!(BAD_TAP.validate().is_err());
    }

    #[test]
    fn the_live_manifest_hosts_the_version_it_replaced() {
        assert!(CLASSIC_3BAND_V2.can_host(&CLASSIC_3BAND));
        assert!(ISOLATOR_3BAND_V2.can_host(&ISOLATOR_3BAND));
    }

    #[test]
    fn a_manifest_cannot_host_one_with_addresses_it_lacks() {
        assert!(!CLASSIC_3BAND.can_host(&CLASSIC_3BAND_V2));
    }

    #[test]
    fn a_manifest_cannot_host_another_units_slot() {
        assert!(!CLASSIC_3BAND_V2.can_host(&ISOLATOR_3BAND));
        assert!(!ISOLATOR_3BAND_V2.can_host(&CLASSIC_3BAND));
    }

    #[test]
    fn a_manifest_cannot_host_one_whose_strip_runs_in_another_order() {
        const FILTER_FIRST_STRIP: &[SlotDescriptor] = &[
            SWEEP_FILTER_SLOT,
            SlotDescriptor {
                slot: "eq",
                unit_id: "eq3band",
                params: &[
                    eq_param("low", "Low", "LO"),
                    eq_param("mid", "Mid", "MD"),
                    eq_param("high", "High", "HI"),
                ],
            },
            FADER_SLOT,
        ];
        const FILTER_FIRST: MixerManifest = MixerManifest {
            id: "filter-first",
            cue_tap: "fader",
            strip: FILTER_FIRST_STRIP,
            master: MASTER_SLOTS,
        };
        assert!(!FILTER_FIRST.can_host(&CLASSIC_3BAND));
        assert!(!CLASSIC_3BAND.can_host(&FILTER_FIRST));
    }

    #[test]
    fn a_manifest_cannot_host_one_that_taps_cue_elsewhere() {
        const CUE_AT_EQ: MixerManifest = MixerManifest {
            id: "cue-at-eq",
            cue_tap: "eq",
            strip: CLASSIC_STRIP,
            master: MASTER_SLOTS,
        };
        assert!(!CUE_AT_EQ.can_host(&CLASSIC_3BAND));
        assert!(!CLASSIC_3BAND.can_host(&CUE_AT_EQ));
    }

    #[test]
    fn every_manifest_hosts_itself() {
        for manifest in MANIFESTS {
            assert!(manifest.can_host(manifest), "{}", manifest.id);
        }
    }

    #[test]
    fn every_mixer_lane_resolves_to_a_descriptor() {
        for manifest in MANIFESTS {
            for lane in EditableLane::ALL {
                // Rate has no slot, and the v1 manifests have no crossfader one.
                if lane.slot_param().is_none() || lane == EditableLane::Xfader {
                    continue;
                }
                assert!(
                    lane.descriptor(manifest).is_some(),
                    "{} addresses a slot/param '{}' does not have",
                    lane.key(),
                    manifest.id
                );
            }
        }
    }

    #[test]
    fn every_mixer_resolves_the_crossfader_lane_to_its_own_range() {
        for manifest in MANIFESTS {
            let spec = lane_spec_for(EditableLane::Xfader, manifest, None, None);
            assert_eq!((spec.min, spec.max), (-1.0, 1.0), "{}", manifest.id);
            assert_eq!(spec.default_value, 0.0, "{}", manifest.id);
            assert_eq!(
                EditableLane::Xfader.display(manifest).short_label,
                "X",
                "{}",
                manifest.id
            );
        }
    }

    #[test]
    fn lane_specs_carry_the_published_ranges() {
        let cases: &[(EditableLane, f64, f64, f64)] = &[
            (EditableLane::Gain, 0.0, 1.0, 1.0),
            (EditableLane::EqLow, -26.0, 6.0, 0.0),
            (EditableLane::EqMid, -26.0, 6.0, 0.0),
            (EditableLane::EqHigh, -26.0, 6.0, 0.0),
            (EditableLane::Filter, -1.0, 1.0, 0.0),
            (EditableLane::Rate, 0.92, 1.08, 1.0),
            (
                EditableLane::MasterGain,
                0.0,
                1.0,
                crate::DEFAULT_MASTER_GAIN as f64,
            ),
        ];
        for (lane, min, max, default) in cases {
            let spec = lane_spec_for(*lane, &CLASSIC_3BAND, None, None);
            assert_eq!(spec.min, *min, "{} min", lane.key());
            assert_eq!(spec.max, *max, "{} max", lane.key());
            assert_eq!(spec.default_value, *default, "{} default", lane.key());
        }
    }

    #[test]
    fn a_lane_takes_its_range_from_the_mixer_it_is_asked_for() {
        let classic = lane_spec_for(EditableLane::EqLow, &CLASSIC_3BAND, None, None);
        let isolator = lane_spec_for(EditableLane::EqLow, &ISOLATOR_3BAND, None, None);

        assert_eq!(
            (classic.min, classic.max),
            (crate::EQ_MIN_DB, crate::EQ_MAX_DB)
        );
        assert_eq!((isolator.min, isolator.max), (0.0, 1.0));
        assert_eq!(classic.default_value, 0.0);
        assert_eq!(isolator.default_value, 1.0);

        // Transport lanes are mixer-independent.
        for manifest in MANIFESTS {
            let rate = lane_spec_for(EditableLane::Rate, manifest, None, None);
            assert_eq!(
                (rate.min, rate.max),
                (0.92, 1.08),
                "rate on {}",
                manifest.id
            );
        }
    }

    #[test]
    fn a_session_without_a_header_replays_on_the_classic_mixer() {
        assert_eq!(
            resolve_manifest(None).expect("no header").id,
            "classic-3band"
        );
    }

    #[test]
    fn a_matching_header_resolves() {
        let header = CLASSIC_3BAND.header();
        assert_eq!(
            resolve_manifest(Some(&header)).expect("matching header").id,
            "classic-3band"
        );
    }

    #[test]
    fn an_unknown_mixer_id_is_refused() {
        let header = MixerHeader {
            id: "isolator".to_string(),
            hash: CLASSIC_3BAND.content_hash(),
        };
        assert!(resolve_manifest(Some(&header)).is_err());
    }

    #[test]
    fn a_changed_mixer_is_refused() {
        let header = MixerHeader {
            id: "classic-3band".to_string(),
            hash: "0000000000000000".to_string(),
        };
        assert!(resolve_manifest(Some(&header)).is_err());
    }

    #[test]
    fn the_hash_tracks_semantics_and_ignores_labels() {
        const RENAMED: MixerManifest = MixerManifest {
            id: "classic-3band",
            cue_tap: "fader",
            strip: &[SlotDescriptor {
                slot: "fader",
                unit_id: "fader",
                params: &[fader_gain_param("Level", "LV", 1.0)],
            }],
            master: &[],
        };
        const RERANGED: MixerManifest = MixerManifest {
            id: "classic-3band",
            cue_tap: "fader",
            strip: &[SlotDescriptor {
                slot: "fader",
                unit_id: "fader",
                params: &[fader_gain_param("Volume", "G", 0.8)],
            }],
            master: &[],
        };
        const ORIGINAL: MixerManifest = MixerManifest {
            id: "classic-3band",
            cue_tap: "fader",
            strip: &[SlotDescriptor {
                slot: "fader",
                unit_id: "fader",
                params: &[fader_gain_param("Volume", "G", 1.0)],
            }],
            master: &[],
        };
        assert_eq!(ORIGINAL.content_hash(), RENAMED.content_hash());
        assert_ne!(ORIGINAL.content_hash(), RERANGED.content_hash());
    }

    #[test]
    fn every_registered_manifest_is_valid_and_uniquely_identified() {
        let mut seen: Vec<&str> = Vec::new();
        for manifest in MANIFESTS {
            manifest
                .validate()
                .unwrap_or_else(|error| panic!("{error}"));
            assert!(!seen.contains(&manifest.id), "duplicate id {}", manifest.id);
            seen.push(manifest.id);
        }
    }

    #[test]
    fn every_lane_has_display_metadata() {
        for manifest in MANIFESTS {
            for lane in EditableLane::ALL {
                let display = lane.display(manifest);
                assert!(
                    !display.short_label.is_empty(),
                    "{} on {}",
                    lane.key(),
                    manifest.id
                );
                assert!(
                    !display.unit.is_empty(),
                    "{} on {}",
                    lane.key(),
                    manifest.id
                );
            }
        }
    }

    #[test]
    fn a_control_position_lands_on_the_params_own_range() {
        let gain = CLASSIC_3BAND
            .descriptor(ParamScope::Deck, "fader", "gain")
            .expect("fader/gain");
        assert_eq!(gain.from_unit_interval(0.0), 0.0);
        assert_eq!(gain.from_unit_interval(1.0), 1.0);
        assert_eq!(gain.from_unit_interval(0.5), 0.5);

        let low = CLASSIC_3BAND
            .descriptor(ParamScope::Deck, "eq", "low")
            .expect("eq/low");
        assert_eq!(low.from_unit_interval(0.0), crate::EQ_MIN_DB);
        assert_eq!(low.from_unit_interval(1.0), crate::EQ_MAX_DB);
    }

    #[test]
    fn a_centred_control_lands_on_a_bipolar_params_centre() {
        let filter = CLASSIC_3BAND
            .descriptor(ParamScope::Deck, "filter", "value")
            .expect("filter/value");
        assert_eq!(filter.from_unit_interval(0.5), 0.0);
    }

    #[test]
    fn a_centred_eq_knob_reads_unity_rather_than_the_middle_of_the_range() {
        let low = CLASSIC_3BAND
            .descriptor(ParamScope::Deck, "eq", "low")
            .expect("eq/low");
        assert_eq!(low.from_unit_interval(0.5), 0.0);
        assert_eq!(low.from_unit_interval(0.25), crate::EQ_MIN_DB / 2.0);
        assert_eq!(low.from_unit_interval(0.75), crate::EQ_MAX_DB / 2.0);
    }

    #[test]
    fn a_bipolar_taper_over_a_symmetric_range_stays_linear() {
        let filter = CLASSIC_3BAND
            .descriptor(ParamScope::Deck, "filter", "value")
            .expect("filter/value");
        for step in 0..=100 {
            let position = f64::from(step) / 100.0;
            let linear = -1.0 + position * 2.0;
            let quantized = (linear / 0.01).round() * 0.01;
            assert!(
                (filter.from_unit_interval(position) - quantized).abs() < 1e-9,
                "at {position}"
            );
        }
    }

    #[test]
    fn a_control_position_is_quantized_to_the_params_step() {
        let low = CLASSIC_3BAND
            .descriptor(ParamScope::Deck, "eq", "low")
            .expect("eq/low");
        // step is 0.5 dB over -26..6, so every result is a multiple of it.
        for step in 0..=127 {
            let value = low.from_unit_interval(step as f64 / 127.0);
            assert_eq!(value, (value / 0.5).round() * 0.5, "at {step}");
            assert!(
                (crate::EQ_MIN_DB..=crate::EQ_MAX_DB).contains(&value),
                "at {step}"
            );
        }
    }

    #[test]
    fn a_position_outside_the_unit_interval_is_clamped() {
        let gain = CLASSIC_3BAND
            .descriptor(ParamScope::Deck, "fader", "gain")
            .expect("fader/gain");
        assert_eq!(gain.from_unit_interval(-1.0), 0.0);
        assert_eq!(gain.from_unit_interval(2.0), 1.0);
    }

    #[test]
    fn the_filter_descriptor_carries_the_dead_zone() {
        let filter = CLASSIC_3BAND
            .descriptor(ParamScope::Deck, "filter", "value")
            .expect("filter/value");
        assert_eq!(filter.dead_zone, Some(crate::FILTER_DEAD_ZONE));
    }
}
