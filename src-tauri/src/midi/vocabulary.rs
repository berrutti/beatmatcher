use super::mapping::Action;

/// What a control has to be for an action to read it. `decode.rs` reaches an
/// action from exactly one of these, so a binding that pairs the wrong two is
/// silently dead at runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ControlKind {
    /// A note, pressed and released.
    Button,
    /// A control change reporting where it sits.
    Positional,
    /// A control change reporting how far it moved.
    Relative,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionSpec {
    pub(super) id: &'static str,
    pub(super) needs_deck: bool,
    /// A mixer address, so the slot and param come from the manifest rather than
    /// from this list.
    pub(super) addressed: bool,
    /// How far one press moves the cursor. An action offered at more than one
    /// value is that many separate bindings, because a press carries its own
    /// distance and the control it is on cannot report one.
    pub(super) steps: Option<i32>,
    pub(super) kind: ControlKind,
    /// `decode.rs` turns this action's release edge into its own move, so a button
    /// that reports only its press leaves the deck holding the press forever.
    pub(super) needs_release: bool,
}

const fn deck_action(id: &'static str, kind: ControlKind) -> ActionSpec {
    ActionSpec {
        id,
        needs_deck: true,
        addressed: false,
        steps: None,
        kind,
        needs_release: false,
    }
}

/// Held rather than latched: the deck acts on the press and undoes it on the release.
const fn held_deck_action(id: &'static str) -> ActionSpec {
    ActionSpec {
        needs_release: true,
        ..deck_action(id, ControlKind::Button)
    }
}

const fn global_action(id: &'static str, kind: ControlKind) -> ActionSpec {
    ActionSpec {
        id,
        needs_deck: false,
        addressed: false,
        steps: None,
        kind,
        needs_release: false,
    }
}

const fn browse(steps: Option<i32>, kind: ControlKind) -> ActionSpec {
    ActionSpec {
        id: "browse",
        needs_deck: false,
        addressed: false,
        steps,
        kind,
        needs_release: false,
    }
}

const BUTTON: ControlKind = ControlKind::Button;
const POSITIONAL: ControlKind = ControlKind::Positional;
const RELATIVE: ControlKind = ControlKind::Relative;

/// Every action a mapping file may name. `parse_mapping` refuses a file naming
/// anything absent from here, so a shipped mapping using an action the learn UI
/// cannot offer fails the built-in mapping tests.
pub(super) const ACTIONS: &[ActionSpec] = &[
    ActionSpec {
        id: "deck_param",
        needs_deck: true,
        addressed: true,
        steps: None,
        kind: POSITIONAL,
        needs_release: false,
    },
    deck_action("play_toggle", BUTTON),
    deck_action("cue_toggle", BUTTON),
    held_deck_action("transport_cue"),
    deck_action("loop_in", BUTTON),
    deck_action("loop_out", BUTTON),
    deck_action("loop_exit_or_reloop", BUTTON),
    deck_action("quantize_toggle", BUTTON),
    held_deck_action("shift"),
    deck_action("eject", BUTTON),
    deck_action("load", BUTTON),
    deck_action("tempo_fader", POSITIONAL),
    deck_action("jog", RELATIVE),
    global_action("xfader_position", POSITIONAL),
    browse(None, RELATIVE),
    browse(Some(-1), BUTTON),
    browse(Some(1), BUTTON),
    global_action("enter", BUTTON),
    global_action("back", BUTTON),
    global_action("toggle_view", BUTTON),
];

pub(super) fn spec(id: &str, steps: Option<i32>) -> Option<&'static ActionSpec> {
    ACTIONS
        .iter()
        .find(|action| action.id == id && action.steps == steps)
        .or_else(|| ACTIONS.iter().find(|action| action.id == id))
}

impl Action {
    /// The name this action carries in a mapping file. Exhaustive, so a new
    /// variant has to be named before it compiles.
    pub(super) fn id(&self) -> &'static str {
        match self {
            Action::DeckParam { .. } => "deck_param",
            Action::CueToggle { .. } => "cue_toggle",
            Action::PlayToggle { .. } => "play_toggle",
            Action::TransportCue { .. } => "transport_cue",
            Action::LoopIn { .. } => "loop_in",
            Action::LoopOut { .. } => "loop_out",
            Action::LoopExitOrReloop { .. } => "loop_exit_or_reloop",
            Action::TempoFader { .. } => "tempo_fader",
            Action::Jog { .. } => "jog",
            Action::Shift { .. } => "shift",
            Action::QuantizeToggle { .. } => "quantize_toggle",
            Action::Eject { .. } => "eject",
            Action::Browse { .. } => "browse",
            Action::Load { .. } => "load",
            Action::Enter => "enter",
            Action::Back => "back",
            Action::ToggleView => "toggle_view",
            Action::XfaderPosition => "xfader_position",
        }
    }
}

/// One mixer address a `deck_param` binding can name.
#[derive(serde::Serialize)]
pub struct ParamAddress {
    pub slot: &'static str,
    pub param: &'static str,
}

/// What the frontend needs to offer a mapping: the closed action list and the
/// mixer addresses the live manifest exposes.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Vocabulary {
    pub actions: &'static [ActionSpec],
    pub deck_params: Vec<ParamAddress>,
}

/// A control change places a value on a range, which a switch does not have, so
/// a bool param is not reachable from a `deck_param` binding.
pub fn vocabulary() -> Vocabulary {
    let deck_params = crate::audio::MIXER
        .strip
        .iter()
        .flat_map(|slot| {
            slot.params
                .iter()
                .filter(|param| !matches!(param.unit, session_core::ParamUnit::Bool))
                .map(move |param| ParamAddress {
                    slot: slot.slot,
                    param: param.id,
                })
        })
        .collect();
    Vocabulary {
        actions: ACTIONS,
        deck_params,
    }
}

#[cfg(test)]
mod tests {
    use super::super::mapping::{built_in_mappings, BindingSpec, DeckScope};
    use super::*;

    fn spec_for(action: &str) -> BindingSpec {
        BindingSpec {
            channel: 1,
            action: action.to_string(),
            note: None,
            cc: Some(1),
            resolution: None,
            button: None,
            deck: Some("A".to_string()),
            slot: Some("eq".to_string()),
            param: Some("low".to_string()),
            steps: None,
        }
    }

    #[test]
    fn every_offered_action_builds_a_binding_that_reports_the_same_id() {
        for action in ACTIONS {
            let built = spec_for(action.id)
                .build(DeckScope::Fixed, None)
                .unwrap_or_else(|error| panic!("{}: {error}", action.id));
            assert_eq!(built.action.id(), action.id);
        }
    }

    #[test]
    fn an_action_needing_a_deck_is_the_one_that_refuses_a_binding_without_one() {
        for action in ACTIONS {
            let built = BindingSpec {
                deck: None,
                ..spec_for(action.id)
            }
            .build(DeckScope::Fixed, None);
            assert_eq!(built.is_err(), action.needs_deck, "{}", action.id);
        }
    }

    #[test]
    fn an_addressed_action_is_the_one_that_refuses_a_binding_without_a_slot() {
        for action in ACTIONS {
            let built = BindingSpec {
                slot: None,
                param: None,
                ..spec_for(action.id)
            }
            .build(DeckScope::Fixed, None);
            assert_eq!(built.is_err(), action.addressed, "{}", action.id);
        }
    }

    #[test]
    fn every_shipped_binding_names_an_action_the_vocabulary_offers() {
        for mapping in built_in_mappings() {
            let profile = mapping.profile(Some("A")).expect(mapping.name());
            for binding in &profile.bindings {
                assert!(
                    spec(binding.action.id(), None).is_some(),
                    "{}: {}",
                    mapping.name(),
                    binding.action.id()
                );
            }
        }
    }

    // The two `decode.rs` turns a release into its own move.
    #[test]
    fn the_actions_that_read_a_release_are_the_held_ones() {
        let held: Vec<&str> = ACTIONS
            .iter()
            .filter(|action| action.needs_release)
            .map(|action| action.id)
            .collect();
        assert_eq!(held, vec!["transport_cue", "shift"]);
    }

    #[test]
    fn a_bool_param_is_not_an_address_a_control_change_can_reach() {
        let offered = vocabulary().deck_params;
        assert!(offered
            .iter()
            .any(|address| (address.slot, address.param) == ("filter", "value")));
        assert!(!offered
            .iter()
            .any(|address| (address.slot, address.param) == ("filter", "active")));
    }
}
