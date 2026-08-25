use super::mapping::{Action, Profile};
use super::wire::{
    relative_delta, unit_interval, Half, Key, Resolution, CONTROL_CHANGE, FOURTEEN_BIT_MAX,
    NOTE_ON, SEVEN_BIT_MAX,
};
use std::collections::HashMap;
#[derive(Default)]
pub(super) struct ControlMemory {
    pub(super) halves: HashMap<usize, (u8, u8)>,
}

impl ControlMemory {
    /// Acts on both halves rather than waiting for a pair, or a controller sending only
    /// the changed half stalls. A lone high half lands within one low-half step.
    pub(super) fn join(&mut self, binding: usize, half: Half, value: u8) -> u16 {
        let halves = self.halves.entry(binding).or_insert((0, 0));
        match half {
            Half::Msb => halves.0 = value,
            Half::Lsb => halves.1 = value,
        }
        (u16::from(halves.0) << 7) | u16::from(halves.1)
    }

    /// A half left over from before a reconnect would join with the first
    /// message after it and produce one value the knob was never at.
    pub(super) fn clear(&mut self) {
        self.halves.clear();
    }
}

pub(super) struct ControlChange {
    pub(super) channel: u8,
    pub(super) controller: u8,
    pub(super) value: u8,
}

pub(super) fn parse_control_change(data: &[u8]) -> Option<ControlChange> {
    let &[status, controller, value] = data else {
        return None;
    };
    if status & 0xF0 != CONTROL_CHANGE {
        return None;
    }
    Some(ControlChange {
        channel: status & 0x0F,
        controller,
        value: value & 0x7F,
    })
}

pub(super) struct NoteOn {
    pub(super) channel: u8,
    pub(super) note: u8,
    pub(super) velocity: u8,
}

pub(super) fn parse_note_on(data: &[u8]) -> Option<NoteOn> {
    let &[status, note, velocity] = data else {
        return None;
    };
    if status & 0xF0 != NOTE_ON {
        return None;
    }
    Some(NoteOn {
        channel: status & 0x0F,
        note,
        velocity: velocity & 0x7F,
    })
}

/// Separated from applying it so the mapping path is testable without an engine.
#[derive(Debug, PartialEq)]
pub(super) enum Move {
    Param {
        deck: String,
        slot: String,
        param: String,
        position: f64,
    },
    Cue {
        deck: String,
    },
    Play {
        deck: String,
    },
    CuePress {
        deck: String,
    },
    CueRelease {
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
    Tempo {
        deck: String,
        position: f64,
    },
    Jog {
        deck: String,
        ticks: i32,
    },
    Shift {
        deck: String,
        held: bool,
    },
    QuantizeToggle {
        deck: String,
    },
    Eject {
        deck: String,
    },
    Browse {
        steps: i32,
    },
    Load {
        deck: String,
    },
    Enter,
    Back,
    ToggleView,
    Xfader {
        position: f64,
    },
}

pub(super) fn resolve_move(
    profile: &Profile,
    memory: &mut ControlMemory,
    data: &[u8],
) -> Option<Move> {
    if let Some(message) = parse_control_change(data) {
        let (index, binding, half) = profile.resolve(Key::ControlChange {
            channel: message.channel,
            controller: message.controller,
        })?;
        // Ahead of the position maths, and ahead of `join`, so a relative control
        // never leaves a half behind for a real pair to collide with.
        if let Some(delta) = relative_delta(binding.source.resolution(), message.value) {
            return match &binding.action {
                Action::Jog { deck } => (delta != 0).then(|| Move::Jog {
                    deck: deck.clone(),
                    ticks: delta,
                }),
                Action::Browse { .. } => (delta != 0).then_some(Move::Browse { steps: delta }),
                _ => None,
            };
        }
        let position = match binding.source.resolution() {
            Resolution::FourteenBit => unit_interval(
                f64::from(memory.join(index, half, message.value)),
                FOURTEEN_BIT_MAX,
            ),
            // A relative control returned above, so it never reaches this. The arm
            // is spelled out anyway so a new resolution has to be considered here.
            Resolution::SevenBit | Resolution::CentreDelta | Resolution::SignedStep => {
                unit_interval(f64::from(message.value), SEVEN_BIT_MAX)
            }
        };
        return match &binding.action {
            Action::DeckParam { deck, slot, param } => Some(Move::Param {
                deck: deck.clone(),
                slot: slot.clone(),
                param: param.clone(),
                position,
            }),
            Action::XfaderPosition => Some(Move::Xfader { position }),
            Action::TempoFader { deck } => Some(Move::Tempo {
                deck: deck.clone(),
                position,
            }),
            Action::CueToggle { .. }
            | Action::PlayToggle { .. }
            | Action::TransportCue { .. }
            | Action::LoopIn { .. }
            | Action::LoopOut { .. }
            | Action::LoopExitOrReloop { .. }
            | Action::Jog { .. }
            | Action::Shift { .. }
            | Action::QuantizeToggle { .. }
            | Action::Eject { .. }
            | Action::Browse { .. }
            | Action::Load { .. }
            | Action::Enter
            | Action::Back
            | Action::ToggleView => None,
        };
    }

    let message = parse_note_on(data)?;
    let (_, binding, _) = profile.resolve(Key::Note {
        channel: message.channel,
        note: message.note,
    })?;
    let pressed = message.velocity > 0;
    match &binding.action {
        // Toggles in the app, momentary buttons on the controller.
        Action::CueToggle { deck } => pressed.then(|| Move::Cue { deck: deck.clone() }),
        Action::PlayToggle { deck } => pressed.then(|| Move::Play { deck: deck.clone() }),
        Action::TransportCue { deck } => Some(if pressed {
            Move::CuePress { deck: deck.clone() }
        } else {
            Move::CueRelease { deck: deck.clone() }
        }),
        Action::LoopIn { deck } => pressed.then(|| Move::LoopIn { deck: deck.clone() }),
        Action::LoopOut { deck } => pressed.then(|| Move::LoopOut { deck: deck.clone() }),
        Action::LoopExitOrReloop { deck } => {
            pressed.then(|| Move::LoopExitOrReloop { deck: deck.clone() })
        }
        Action::Shift { deck } => Some(Move::Shift {
            deck: deck.clone(),
            held: pressed,
        }),
        // Momentary on the surface, latching in the app, so only the press turns
        // it over. The release would turn it straight back.
        Action::QuantizeToggle { deck } => {
            pressed.then(|| Move::QuantizeToggle { deck: deck.clone() })
        }
        Action::Eject { deck } => pressed.then(|| Move::Eject { deck: deck.clone() }),
        Action::Load { deck } => pressed.then(|| Move::Load { deck: deck.clone() }),
        Action::Enter => pressed.then_some(Move::Enter),
        Action::Back => pressed.then_some(Move::Back),
        Action::ToggleView => pressed.then_some(Move::ToggleView),
        Action::Browse { steps } => steps
            .filter(|_| pressed)
            .map(|steps| Move::Browse { steps }),
        Action::DeckParam { .. }
        | Action::XfaderPosition
        | Action::TempoFader { .. }
        | Action::Jog { .. } => None,
    }
}
