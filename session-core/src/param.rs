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

pub enum ParamKind {
    Knob,
    Fader,
    Toggle,
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
    pub kind: ParamKind,
    pub automatable: bool,
    pub lane_group: u8,
}

impl ParamDescriptor {
    /// Places a normalized control position on this param's own range. The
    /// quantization to `step` is what keeps a high-resolution controller from
    /// emitting far more events than the step-limited UI inputs ever would.
    ///
    /// A bipolar param gives each side of its centre half the travel, so a
    /// detented knob reads unity at the detent even when the range around it is
    /// lopsided. That is what a mixer's EQ pot does, and the cost is that the
    /// two halves move at different rates: -26..0 over the lower half is far
    /// steeper than 0..+6 over the upper.
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

    pub fn descriptor(&self, scope: ParamScope, slot: &str, param: &str) -> Option<&ParamDescriptor> {
        match scope {
            ParamScope::Deck => self.strip_slot(slot)?.param(param),
            ParamScope::Master => self.master_slot(slot)?.param(param),
        }
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

pub static MANIFESTS: &[&MixerManifest] = &[&CLASSIC_3BAND, &ISOLATOR_3BAND];

pub fn manifest_by_id(id: &str) -> Option<&'static MixerManifest> {
    MANIFESTS.iter().copied().find(|manifest| manifest.id == id)
}

/// What a `.bms` header records about the mixer a session was played on.
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
    let manifest = manifest_by_id(&header.id)
        .ok_or_else(|| format!("session needs mixer '{}', which this build does not have", header.id))?;
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
    bytes
        .iter()
        .fold(hash, |acc, byte| (acc ^ *byte as u64).wrapping_mul(FNV_PRIME))
}

impl MixerManifest {
    /// Identifies the manifest by everything that changes what a `set_param`
    /// event means. Labels and lane grouping are excluded: renaming a knob must
    /// not invalidate sessions recorded against it.
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
                    hash = fnv_bytes(hash, &param.dead_zone.unwrap_or(f64::NAN).to_bits().to_le_bytes());
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
    strip: &[
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
    ],
    master: MASTER_SLOTS,
};

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
            kind: ParamKind::Knob,
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
            kind: ParamKind::Toggle,
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
        kind: ParamKind::Fader,
        automatable: true,
        lane_group: 0,
    }],
};

const MASTER_SLOTS: &[SlotDescriptor] = &[SlotDescriptor {
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
        kind: ParamKind::Fader,
        automatable: true,
        lane_group: 0,
    }],
}];

// The same strip as the classic mixer with a different unit in the `eq` slot:
// full kill instead of a shelf, so the params share their ids but not their
// range. The manifest hash is what stops a session recorded on one from being
// rendered on the other.
pub static ISOLATOR_3BAND: MixerManifest = MixerManifest {
    id: "isolator-3band",
    cue_tap: "fader",
    strip: &[
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
    ],
    master: MASTER_SLOTS,
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
        kind: ParamKind::Knob,
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
        kind: ParamKind::Knob,
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
            kind: ParamKind::Fader,
            automatable: true,
            lane_group: 0,
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
                    kind: ParamKind::Fader,
                    automatable: true,
                    lane_group: 0,
                }],
            }],
            master: &[],
        };
        assert!(BAD_TAP.validate().is_err());
    }

    #[test]
    fn every_mixer_lane_resolves_to_a_descriptor() {
        for manifest in MANIFESTS {
            for lane in EditableLane::ALL {
                if lane.slot_param().is_none() {
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

    // The ranges the editor and the mixer UI publish. Pinned as literals so a
    // descriptor edit has to be deliberate.
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

    // The editor draws and clamps eq lanes against these, so reading them off
    // the wrong manifest would put the curve in the wrong place and clamp an
    // edit to a range the session never had.
    #[test]
    fn a_lane_takes_its_range_from_the_mixer_it_is_asked_for() {
        let classic = lane_spec_for(EditableLane::EqLow, &CLASSIC_3BAND, None, None);
        let isolator = lane_spec_for(EditableLane::EqLow, &ISOLATOR_3BAND, None, None);

        assert_eq!((classic.min, classic.max), (crate::EQ_MIN_DB, crate::EQ_MAX_DB));
        assert_eq!((isolator.min, isolator.max), (0.0, 1.0));
        assert_eq!(classic.default_value, 0.0);
        assert_eq!(isolator.default_value, 1.0);

        // Transport lanes are mixer-independent.
        for manifest in MANIFESTS {
            let rate = lane_spec_for(EditableLane::Rate, manifest, None, None);
            assert_eq!((rate.min, rate.max), (0.92, 1.08), "rate on {}", manifest.id);
        }
    }

    #[test]
    fn a_session_without_a_header_replays_on_the_classic_mixer() {
        assert_eq!(resolve_manifest(None).expect("no header").id, "classic-3band");
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

    // The whole point of storing a hash: rendering a session against a mixer
    // that has since changed shape would silently diverge from the recording.
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
            manifest.validate().unwrap_or_else(|error| panic!("{error}"));
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
                assert!(!display.unit.is_empty(), "{} on {}", lane.key(), manifest.id);
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

    // A bipolar param's centre is the value the dead zone is measured around, so
    // a controller sitting at its detent has to produce exactly that.
    #[test]
    fn a_centred_control_lands_on_a_bipolar_params_centre() {
        let filter = CLASSIC_3BAND
            .descriptor(ParamScope::Deck, "filter", "value")
            .expect("filter/value");
        assert_eq!(filter.from_unit_interval(0.5), 0.0);
    }

    // The reason eq is bipolar at all: a detented knob reports its centre, and
    // that has to be unity rather than the -10 dB a linear map over -26..+6
    // would give.
    #[test]
    fn a_centred_eq_knob_reads_unity_rather_than_the_middle_of_the_range() {
        let low = CLASSIC_3BAND
            .descriptor(ParamScope::Deck, "eq", "low")
            .expect("eq/low");
        assert_eq!(low.from_unit_interval(0.5), 0.0);
        assert_eq!(low.from_unit_interval(0.25), crate::EQ_MIN_DB / 2.0);
        assert_eq!(low.from_unit_interval(0.75), crate::EQ_MAX_DB / 2.0);
    }

    // A symmetric range is the case where a bipolar taper and a linear one
    // agree, which is why the filter's behaviour is untouched by this.
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
            assert!((crate::EQ_MIN_DB..=crate::EQ_MAX_DB).contains(&value), "at {step}");
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
