use super::deck::ChannelStrip;
use super::dsp::Reverb;
use super::unit::StripSettings;
use crate::lock::LockIgnoringPoison;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReverbSettings {
    pub tail_ms: f32,
}

impl ReverbSettings {
    pub(crate) fn reverb(&self, sample_rate: f32) -> Reverb {
        let mut reverb = Reverb::new(sample_rate);
        self.apply_to(&mut reverb);
        reverb.settle();
        reverb
    }

    pub(crate) fn apply_to(&self, reverb: &mut Reverb) {
        reverb.set_tail_ms(self.tail_ms);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MasterSettings {
    pub reverb: ReverbSettings,
    pub strips: StripSettings,
}

impl MasterSettings {
    pub fn for_manifest(manifest: &session_core::MixerManifest) -> Self {
        let mut settings = Self {
            reverb: ReverbSettings::default(),
            strips: StripSettings::default(),
        };
        for slot in manifest.master {
            for param in slot.params {
                settings.set(slot.slot, param.id, param.default as f32);
            }
        }
        settings
    }

    pub fn get(&self, slot: &str, param: &str) -> Option<f32> {
        match (slot, param) {
            ("reverb", "tail") => Some(self.reverb.tail_ms),
            ("filter", "resonance") => Some(self.strips.filter_resonance),
            _ => None,
        }
    }

    pub fn set(&mut self, slot: &str, param: &str, value: f32) -> bool {
        let field = match (slot, param) {
            ("reverb", "tail") => &mut self.reverb.tail_ms,
            ("filter", "resonance") => &mut self.strips.filter_resonance,
            _ => return false,
        };
        *field = value;
        true
    }
}

/// One switch, every channel, as on a mixer: the strips each hold their own
/// copy because the taper is applied before the fader's own smoothing.
pub(crate) fn apply_to_strips(
    strips: &HashMap<String, Arc<Mutex<ChannelStrip>>>,
    settings: &StripSettings,
) {
    for strip in strips.values() {
        strip.locked().apply_master(settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_effect(slot: &str) -> bool {
        !matches!(slot, "gain" | "xfader")
    }

    #[test]
    fn every_master_effect_a_mixer_declares_has_a_home() {
        for manifest in session_core::MANIFESTS {
            for slot in manifest.master.iter().filter(|slot| is_effect(slot.slot)) {
                for param in slot.params {
                    let mut settings = MasterSettings::for_manifest(manifest);
                    assert!(
                        settings.set(slot.slot, param.id, param.max as f32),
                        "{}: {}/{} has nowhere to go",
                        manifest.id,
                        slot.slot,
                        param.id
                    );
                    assert_eq!(settings.get(slot.slot, param.id), Some(param.max as f32));
                }
            }
        }
    }

    #[test]
    fn every_master_effect_starts_at_its_declared_default() {
        for manifest in session_core::MANIFESTS {
            let settings = MasterSettings::for_manifest(manifest);
            for slot in manifest.master.iter().filter(|slot| is_effect(slot.slot)) {
                for param in slot.params {
                    assert_eq!(
                        settings.get(slot.slot, param.id),
                        Some(param.default as f32),
                        "{}: {}/{}",
                        manifest.id,
                        slot.slot,
                        param.id
                    );
                }
            }
        }
    }

    #[test]
    fn gain_and_the_crossfader_are_not_effects() {
        let mut settings = MasterSettings::for_manifest(&session_core::CLASSIC_4BAND);
        assert!(!settings.set("gain", "gain", 0.5));
        assert!(!settings.set("xfader", "position", 0.5));
    }

    fn swept_tone(strip: &mut ChannelStrip) -> Vec<f32> {
        strip.set_param("filter", "value", -0.5);
        strip.set_param("filter", "active", 1.0);
        (0..4_800)
            .map(|frame| {
                let sample = (std::f32::consts::TAU * 100.0 * frame as f32 / 48_000.0).sin();
                strip.process_main(sample, sample).0
            })
            .collect()
    }

    #[test]
    fn the_strip_settings_reach_every_strip() {
        let strips: HashMap<String, Arc<Mutex<ChannelStrip>>> = ["A", "B"]
            .into_iter()
            .map(|id| {
                (
                    id.to_string(),
                    Arc::new(Mutex::new(ChannelStrip::new(48_000.0))),
                )
            })
            .collect();
        let settings = StripSettings {
            filter_resonance: 4.0,
            ..StripSettings::default()
        };

        apply_to_strips(&strips, &settings);

        let mut expected = ChannelStrip::new(48_000.0);
        expected.apply_master(&settings);
        let expected = swept_tone(&mut expected);
        for strip in strips.values() {
            assert_eq!(swept_tone(&mut strip.locked()), expected);
        }
    }
}
