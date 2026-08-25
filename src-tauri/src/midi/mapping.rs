use super::wire::{Half, Key, Resolution, Source};
use std::collections::HashMap;
/// What a lit button reports. A binding is an input address. This says which of
/// them the app also writes back to, so a control with no entry stays dark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Feedback {
    Cue,
    Quantize,
}

pub(super) enum Action {
    DeckParam {
        deck: String,
        slot: String,
        param: String,
    },
    CueToggle {
        deck: String,
    },
    PlayToggle {
        deck: String,
    },
    TransportCue {
        deck: String,
    },
    LoopIn {
        deck: String,
    },
    LoopOut {
        deck: String,
    },
    LoopExitOrReloop {
        deck: String,
    },
    TempoFader {
        deck: String,
    },
    Jog {
        deck: String,
    },
    // Held, not latched: the surface sends both edges and the deck it names is
    // the one whose wheel it modifies.
    Shift {
        deck: String,
    },
    QuantizeToggle {
        deck: String,
    },
    Eject {
        deck: String,
    },
    // `steps` is how far one press moves the cursor, for a surface that browses with buttons.
    // An encoder carries its own delta and leaves this unset.
    Browse {
        steps: Option<i32>,
    },
    Load {
        deck: String,
    },
    Enter,
    Back,
    ToggleView,
    // Master scope, so it names no deck and takes its range from the master
    // descriptor rather than a strip's.
    XfaderPosition,
}

pub(super) struct Binding {
    pub(super) source: Source,
    pub(super) action: Action,
}

pub struct Profile {
    pub(super) bindings: Vec<Binding>,
    pub(super) by_key: HashMap<Key, (usize, Half)>,
    pub(super) led_keys: HashMap<(Feedback, String), Key>,
}

impl Profile {
    /// Refuses a colliding profile rather than letting the last binding win, because the
    /// symptom is the wrong deck's control moving, which reads as broken hardware.
    pub(super) fn new(bindings: Vec<Binding>) -> Result<Self, String> {
        let mut by_key = HashMap::new();
        let mut led_keys = HashMap::new();
        for (index, binding) in bindings.iter().enumerate() {
            let keys = binding.source.keys()?;
            let lit = match &binding.action {
                Action::CueToggle { deck } => Some((Feedback::Cue, deck)),
                Action::QuantizeToggle { deck } => Some((Feedback::Quantize, deck)),
                _ => None,
            };
            if let Some((feedback, deck)) = lit {
                if let Some((key, _)) = keys.first() {
                    led_keys.insert((feedback, deck.clone()), *key);
                }
            }
            for (key, half) in keys {
                if by_key.insert(key, (index, half)).is_some() {
                    return Err(format!("two bindings share {key:?}"));
                }
            }
        }
        Ok(Self {
            bindings,
            by_key,
            led_keys,
        })
    }

    pub(super) fn resolve(&self, key: Key) -> Option<(usize, &Binding, Half)> {
        let &(index, half) = self.by_key.get(&key)?;
        Some((index, &self.bindings[index], half))
    }
}

/// Bumped only when the vocabulary changes. A file declaring a newer version is
/// refused rather than half-read.
pub(super) const MAPPING_VERSION: u32 = 2;

/// The version that introduced the deck template. A file using it is refused below this,
/// because an older build finds no `bindings` at all and presents a dead controller.
pub(super) const DECK_TEMPLATE_VERSION: u32 = 2;

pub(super) const MAPPING_FILES: [&str; 2] = [
    include_str!("../../mappings/ddj-flx6.json"),
    include_str!("../../mappings/xdj-1000mk2.json"),
];

#[derive(serde::Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum DeckScope {
    Fixed,
    Assigned,
}

#[derive(serde::Deserialize, Clone, Copy)]
pub(super) enum ResolutionSpec {
    #[serde(rename = "7bit")]
    SevenBit,
    #[serde(rename = "14bit")]
    FourteenBit,
    #[serde(rename = "centre_delta")]
    CentreDelta,
    #[serde(rename = "signed_step")]
    SignedStep,
}

#[derive(serde::Deserialize)]
pub(super) struct BindingSpec {
    pub(super) channel: u8,
    pub(super) action: String,
    #[serde(default)]
    pub(super) note: Option<u8>,
    #[serde(default)]
    pub(super) cc: Option<u8>,
    #[serde(default)]
    pub(super) resolution: Option<ResolutionSpec>,
    #[serde(default)]
    pub(super) deck: Option<String>,
    #[serde(default)]
    pub(super) slot: Option<String>,
    #[serde(default)]
    pub(super) param: Option<String>,
    #[serde(default)]
    pub(super) steps: Option<i32>,
}

/// One deck's channel on a surface that lays the same controls out per deck.
#[derive(serde::Deserialize)]
pub(super) struct DeckChannel {
    pub(super) deck: String,
    pub(super) channel: u8,
}

/// Written once and expanded across `per_deck`, so the deck and the channel come from the
/// template rather than being retyped per copy where a slip is a valid other-deck address.
#[derive(serde::Deserialize)]
pub(super) struct DeckBindingSpec {
    pub(super) action: String,
    #[serde(default)]
    pub(super) note: Option<u8>,
    #[serde(default)]
    pub(super) cc: Option<u8>,
    #[serde(default)]
    pub(super) resolution: Option<ResolutionSpec>,
    #[serde(default)]
    pub(super) slot: Option<String>,
    #[serde(default)]
    pub(super) param: Option<String>,
    #[serde(default)]
    pub(super) steps: Option<i32>,
}

impl DeckBindingSpec {
    pub(super) fn expand(&self, over: &DeckChannel) -> BindingSpec {
        BindingSpec {
            channel: over.channel,
            action: self.action.clone(),
            note: self.note,
            cc: self.cc,
            resolution: self.resolution,
            deck: Some(over.deck.clone()),
            slot: self.slot.clone(),
            param: self.param.clone(),
            steps: self.steps,
        }
    }
}

pub(super) struct Mapping {
    pub(super) name: String,
    pub(super) matches: Vec<String>,
    pub(super) decks: DeckScope,
    pub(super) bindings: Vec<BindingSpec>,
}

#[derive(serde::Deserialize)]
pub(super) struct MappingFile {
    pub(super) version: u32,
    pub(super) name: String,
    #[serde(rename = "match")]
    pub(super) matches: Vec<String>,
    pub(super) decks: DeckScope,
    #[serde(default)]
    pub(super) per_deck: Vec<DeckChannel>,
    #[serde(default)]
    pub(super) deck_bindings: Vec<DeckBindingSpec>,
    #[serde(default)]
    pub(super) bindings: Vec<BindingSpec>,
}

impl BindingSpec {
    /// `assigned` is the deck the user gave the device, and is what an `assigned` mapping's
    /// bindings are built against. A `fixed` mapping names its own decks and ignores it.
    pub(super) fn deck(&self, scope: DeckScope, assigned: Option<&str>) -> Result<String, String> {
        match scope {
            DeckScope::Fixed => self
                .deck
                .clone()
                .ok_or_else(|| format!("'{}' needs a deck", self.action)),
            DeckScope::Assigned => assigned
                .map(str::to_string)
                .ok_or_else(|| "the device has no deck".to_string()),
        }
    }

    pub(super) fn source(&self) -> Result<Source, String> {
        // The file counts channels the way the hardware's documentation and the
        // console monitor do, from one.
        let channel = self
            .channel
            .checked_sub(1)
            .ok_or_else(|| "channel is counted from 1".to_string())?;
        match (self.note, self.cc) {
            (Some(note), None) => Ok(Source::Note { channel, note }),
            (None, Some(controller)) => Ok(Source::ControlChange {
                channel,
                controller,
                resolution: self
                    .resolution
                    .map_or(Resolution::SevenBit, Resolution::from),
            }),
            _ => Err("a binding needs exactly one of 'note' or 'cc'".to_string()),
        }
    }

    pub(super) fn build(
        &self,
        scope: DeckScope,
        assigned: Option<&str>,
    ) -> Result<Binding, String> {
        let action = match self.action.as_str() {
            "deck_param" => Action::DeckParam {
                deck: self.deck(scope, assigned)?,
                slot: self
                    .slot
                    .clone()
                    .ok_or_else(|| "'deck_param' needs a slot".to_string())?,
                param: self
                    .param
                    .clone()
                    .ok_or_else(|| "'deck_param' needs a param".to_string())?,
            },
            "cue_toggle" => Action::CueToggle {
                deck: self.deck(scope, assigned)?,
            },
            "play_toggle" => Action::PlayToggle {
                deck: self.deck(scope, assigned)?,
            },
            "transport_cue" => Action::TransportCue {
                deck: self.deck(scope, assigned)?,
            },
            "loop_in" => Action::LoopIn {
                deck: self.deck(scope, assigned)?,
            },
            "loop_out" => Action::LoopOut {
                deck: self.deck(scope, assigned)?,
            },
            "loop_exit_or_reloop" => Action::LoopExitOrReloop {
                deck: self.deck(scope, assigned)?,
            },
            "tempo_fader" => Action::TempoFader {
                deck: self.deck(scope, assigned)?,
            },
            "jog" => Action::Jog {
                deck: self.deck(scope, assigned)?,
            },
            "shift" => Action::Shift {
                deck: self.deck(scope, assigned)?,
            },
            "quantize_toggle" => Action::QuantizeToggle {
                deck: self.deck(scope, assigned)?,
            },
            "eject" => Action::Eject {
                deck: self.deck(scope, assigned)?,
            },
            "load" => Action::Load {
                deck: self.deck(scope, assigned)?,
            },
            "browse" => Action::Browse { steps: self.steps },
            "enter" => Action::Enter,
            "back" => Action::Back,
            "toggle_view" => Action::ToggleView,
            "xfader_position" => Action::XfaderPosition,
            other => return Err(format!("unknown action '{other}'")),
        };
        Ok(Binding {
            source: self.source()?,
            action,
        })
    }
}

impl Mapping {
    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn claims(&self, port: &str) -> bool {
        self.matches.iter().any(|needle| port.contains(needle))
    }

    pub(super) fn needs_deck(&self) -> bool {
        self.decks == DeckScope::Assigned
    }

    pub(super) fn profile(&self, assigned: Option<&str>) -> Result<Profile, String> {
        let bindings = self
            .bindings
            .iter()
            .map(|spec| spec.build(self.decks, assigned))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("{}: {error}", self.name))?;
        Profile::new(bindings).map_err(|error| format!("{}: {error}", self.name))
    }
}

pub(super) fn parse_mapping(source: &str) -> Result<Mapping, String> {
    let file: MappingFile = serde_json::from_str(source).map_err(|error| error.to_string())?;
    if file.version > MAPPING_VERSION {
        return Err(format!(
            "{} declares version {}, newer than {MAPPING_VERSION}",
            file.name, file.version
        ));
    }
    let templated = !file.per_deck.is_empty() || !file.deck_bindings.is_empty();
    if templated && file.version < DECK_TEMPLATE_VERSION {
        return Err(format!(
            "{} uses a deck template but declares version {}, older than {DECK_TEMPLATE_VERSION}",
            file.name, file.version
        ));
    }
    if !file.deck_bindings.is_empty() && file.per_deck.is_empty() {
        return Err(format!("{}: 'deck_bindings' needs 'per_deck'", file.name));
    }
    let mut bindings = file.bindings;
    for over in &file.per_deck {
        bindings.extend(file.deck_bindings.iter().map(|spec| spec.expand(over)));
    }
    Ok(Mapping {
        name: file.name,
        matches: file.matches,
        decks: file.decks,
        bindings,
    })
}

pub(super) fn built_in_mappings() -> Vec<Mapping> {
    MAPPING_FILES
        .iter()
        .map(|source| parse_mapping(source).expect("a built-in mapping file"))
        .collect()
}
