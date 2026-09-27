use super::deck::ChannelStrip;
use super::dsp::{Echo, FILTER_RESONANCE_Q};
use crate::lock::LockIgnoringPoison;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EchoSettings {
    pub time_ms: f32,
    pub feedback: f32,
    pub return_level: f32,
    pub low_cut_hz: f32,
}

impl EchoSettings {
    pub(crate) fn echo(&self, sample_rate: f32) -> Echo {
        let mut echo = Echo::new(sample_rate);
        self.apply_to(&mut echo);
        echo.settle();
        echo
    }

    pub(crate) fn apply_to(&self, echo: &mut Echo) {
        echo.set_time_ms(self.time_ms);
        echo.set_feedback(self.feedback);
        echo.set_return(self.return_level);
        echo.set_low_cut(self.low_cut_hz);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MasterEffects {
    pub echo: EchoSettings,
    pub filter_resonance: f32,
}

impl MasterEffects {
    pub fn for_manifest(manifest: &session_core::MixerManifest) -> Self {
        let mut effects = Self {
            echo: EchoSettings {
                time_ms: 0.0,
                feedback: 0.0,
                return_level: 0.0,
                low_cut_hz: 0.0,
            },
            filter_resonance: FILTER_RESONANCE_Q,
        };
        for slot in manifest.master {
            for param in slot.params {
                effects.set(slot.slot, param.id, param.default as f32);
            }
        }
        effects
    }

    fn address_mut(&mut self, slot: &str, param: &str) -> Option<&mut f32> {
        match (slot, param) {
            ("echo", "time") => Some(&mut self.echo.time_ms),
            ("echo", "feedback") => Some(&mut self.echo.feedback),
            ("echo", "return") => Some(&mut self.echo.return_level),
            ("echo", "lowCut") => Some(&mut self.echo.low_cut_hz),
            ("filter", "resonance") => Some(&mut self.filter_resonance),
            _ => None,
        }
    }

    pub fn get(&self, slot: &str, param: &str) -> Option<f32> {
        let mut copy = *self;
        copy.address_mut(slot, param).map(|value| *value)
    }

    pub fn set(&mut self, slot: &str, param: &str, value: f32) -> bool {
        let Some(address) = self.address_mut(slot, param) else {
            return false;
        };
        *address = value;
        true
    }
}

pub(crate) fn apply_master_effect(
    effects: &mut MasterEffects,
    strips: &HashMap<String, Arc<Mutex<ChannelStrip>>>,
    slot: &str,
    param: &str,
    value: f32,
) -> bool {
    if !effects.set(slot, param, value) {
        return false;
    }
    if (slot, param) == ("filter", "resonance") {
        for strip in strips.values() {
            strip.locked().set_filter_resonance(value);
        }
    }
    true
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
                    let mut effects = MasterEffects::for_manifest(manifest);
                    assert!(
                        effects.set(slot.slot, param.id, param.max as f32),
                        "{}: {}/{} has nowhere to go",
                        manifest.id,
                        slot.slot,
                        param.id
                    );
                    assert_eq!(effects.get(slot.slot, param.id), Some(param.max as f32));
                }
            }
        }
    }

    #[test]
    fn every_master_effect_starts_at_its_declared_default() {
        for manifest in session_core::MANIFESTS {
            let effects = MasterEffects::for_manifest(manifest);
            for slot in manifest.master.iter().filter(|slot| is_effect(slot.slot)) {
                for param in slot.params {
                    assert_eq!(
                        effects.get(slot.slot, param.id),
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
        let mut effects = MasterEffects::for_manifest(&session_core::CLASSIC_4BAND);
        assert!(!effects.set("gain", "gain", 0.5));
        assert!(!effects.set("xfader", "position", 0.5));
    }

    #[test]
    fn the_resonance_reaches_every_strip() {
        let strips: HashMap<String, Arc<Mutex<ChannelStrip>>> = ["A", "B"]
            .into_iter()
            .map(|id| {
                (
                    id.to_string(),
                    Arc::new(Mutex::new(ChannelStrip::new(48_000.0))),
                )
            })
            .collect();
        let mut effects = MasterEffects::for_manifest(&session_core::CLASSIC_4BAND);

        assert!(apply_master_effect(
            &mut effects,
            &strips,
            "filter",
            "resonance",
            4.0
        ));

        for strip in strips.values() {
            assert_eq!(strip.locked().master().filter_resonance, 4.0);
        }
    }
}
