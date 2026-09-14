use super::mapping::{BindingSpec, ButtonSpec, DeckScope, Mapping, ResolutionSpec};
use super::vocabulary::{spec, ControlKind};
use super::wire::{LSB_OFFSET, MAX_CONTROLLER};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A control has to stop moving for this long before its capture lands, so a
/// fader is read from the whole sweep rather than from its first message.
const SETTLE: Duration = Duration::from_millis(250);

/// How many values one address keeps. A sweep is decided by its spread, and a
/// 14-bit fader sends thousands.
const WATCH_CAPACITY: usize = 64;

/// One action, addressed the way a mapping file addresses it. Two slots that
/// differ only by deck are two bindings.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub action: String,
    #[serde(default)]
    pub deck: Option<String>,
    #[serde(default)]
    pub slot: Option<String>,
    #[serde(default)]
    pub param: Option<String>,
    /// Part of the address, not a setting: `browse` back and `browse` forward are
    /// one action reached two ways, and a binding names which.
    #[serde(default)]
    pub steps: Option<i32>,
}

impl Slot {
    fn of(spec: &BindingSpec) -> Self {
        Self {
            action: spec.action.clone(),
            deck: spec.deck.clone(),
            slot: spec.slot.clone(),
            param: spec.param.clone(),
            steps: spec.steps,
        }
    }
}

/// What the surface sends for a slot. Channels count from 1 here, the way the
/// file does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capture {
    pub channel: u8,
    pub note: Option<u8>,
    pub cc: Option<u8>,
    pub resolution: Option<ResolutionSpec>,
    /// Read from whether the press was followed by a release while the slot was
    /// armed, so a pad that only reports its press is recorded as one.
    pub button: Option<ButtonSpec>,
}

impl Capture {
    fn of(spec: &BindingSpec) -> Self {
        Self {
            channel: spec.channel,
            note: spec.note,
            cc: spec.cc,
            resolution: spec.resolution,
            button: spec.button,
        }
    }

    fn into_spec(self, slot: &Slot) -> BindingSpec {
        BindingSpec {
            channel: self.channel,
            action: slot.action.clone(),
            note: self.note,
            cc: self.cc,
            resolution: self.resolution,
            button: self.button,
            deck: slot.deck.clone(),
            slot: slot.slot.clone(),
            param: slot.param.clone(),
            steps: slot.steps,
        }
    }
}

#[derive(Default)]
struct Watch {
    notes: Vec<(u8, u8)>,
    released: Vec<(u8, u8)>,
    controls: HashMap<(u8, u8), Vec<u8>>,
}

impl Watch {
    fn note(&mut self, channel: u8, note: u8) {
        if !self.notes.contains(&(channel, note)) {
            self.notes.push((channel, note));
        }
    }

    /// Only for a note already pressed here, so a release left over from before the
    /// slot was armed cannot make a trigger look momentary.
    fn release(&mut self, channel: u8, note: u8) {
        if self.notes.contains(&(channel, note)) && !self.released.contains(&(channel, note)) {
            self.released.push((channel, note));
        }
    }

    fn control(&mut self, channel: u8, controller: u8, value: u8) {
        let values = self.controls.entry((channel, controller)).or_default();
        if values.len() < WATCH_CAPACITY {
            values.push(value);
        }
    }

    /// The one place a message becomes a watched address, so nothing can observe a
    /// stream differently from the way an armed slot does. `false` for a message
    /// that carries no address.
    fn observe(&mut self, data: &[u8]) -> bool {
        if let Some(message) = super::decode::parse_control_change(data) {
            self.control(message.channel, message.controller, message.value);
        } else if let Some(message) = super::decode::parse_note_on(data) {
            // The release does not settle a capture on its own, but whether one
            // arrives at all is what tells a momentary button from a trigger.
            if message.velocity == 0 {
                self.release(message.channel, message.note);
            } else {
                self.note(message.channel, message.note);
            }
        } else {
            return false;
        }
        true
    }

    fn is_empty(&self) -> bool {
        self.notes.is_empty() && self.controls.is_empty()
    }

    /// The address that moved most, so a controller streaming an unrelated value
    /// on another cc does not win over the one being held.
    fn busiest_control(&self) -> Option<(u8, u8)> {
        self.controls
            .iter()
            .max_by_key(|(address, values)| (values.len(), std::cmp::Reverse(**address)))
            .map(|(address, _)| *address)
    }
}

/// One detent either way, which is 1 and 127 and nothing between.
fn only_single_steps(values: &[u8]) -> bool {
    values.iter().all(|value| *value == 1 || *value == 127)
}

fn relative_resolution(values: &[u8]) -> ResolutionSpec {
    if only_single_steps(values) {
        ResolutionSpec::SignedStep
    } else {
        ResolutionSpec::CentreDelta
    }
}

/// A high-resolution control puts its low half `LSB_OFFSET` above its high one,
/// so both halves arriving on one channel is what tells the two apart.
fn positional_resolution(watch: &Watch, channel: u8, controller: u8) -> (u8, ResolutionSpec) {
    let paired = |high: u8| {
        high.checked_add(LSB_OFFSET)
            .filter(|low| *low <= MAX_CONTROLLER)
            .is_some_and(|low| watch.controls.contains_key(&(channel, low)))
    };
    if paired(controller) {
        return (controller, ResolutionSpec::FourteenBit);
    }
    // The busiest address is the low half when a controller sends more of it than
    // of the high one, and the file names the high half.
    if let Some(high) = controller.checked_sub(LSB_OFFSET) {
        if watch.controls.contains_key(&(channel, high)) {
            return (high, ResolutionSpec::FourteenBit);
        }
    }
    (controller, ResolutionSpec::SevenBit)
}

/// `None` while nothing the action can read has arrived, so a button press on a
/// slot expecting a fader leaves the slot armed rather than binding the press.
fn infer(kind: ControlKind, watch: &Watch) -> Option<Capture> {
    if kind == ControlKind::Button {
        let &(channel, note) = watch.notes.first()?;
        return Some(Capture {
            channel: channel + 1,
            note: Some(note),
            cc: None,
            resolution: None,
            button: Some(if watch.released.contains(&(channel, note)) {
                ButtonSpec::Momentary
            } else {
                ButtonSpec::Trigger
            }),
        });
    }
    let (channel, controller) = watch.busiest_control()?;
    let (controller, resolution) = match kind {
        ControlKind::Relative => (
            controller,
            relative_resolution(watch.controls.get(&(channel, controller))?),
        ),
        ControlKind::Positional => positional_resolution(watch, channel, controller),
        ControlKind::Button => return None,
    };
    Some(Capture {
        channel: channel + 1,
        note: None,
        cc: Some(controller),
        resolution: Some(resolution),
        button: None,
    })
}

/// A struct rather than a `json!`, because `serde_json::Value` sorts its keys and a
/// contributed file is read and reviewed header first.
#[derive(serde::Serialize)]
pub struct MappingFile {
    pub version: u32,
    pub name: String,
    #[serde(rename = "match")]
    pub matches: Vec<String>,
    pub decks: DeckScope,
    pub bindings: Vec<BindingSpec>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftBinding {
    pub slot: Slot,
    /// More than one when a modifier re-addresses the control, the way SHIFT moves
    /// the jog to a second cc. Editing the slot replaces all of them.
    pub captures: Vec<Capture>,
}

/// Vocabulary order, then deck and address, so two people mapping one device
/// produce files that diff by what differs rather than by capture order.
type SlotOrder = (usize, Option<String>, Option<String>, Option<String>);

/// Where a mapping that plays one deck is read onto, so the panel has a tab to show
/// its bindings under.
const FIRST_DECK: &str = "A";

fn needs_deck(slot: &Slot) -> bool {
    super::vocabulary::spec(&slot.action, slot.steps).is_some_and(|spec| spec.needs_deck)
}

fn slot_order(slot: &Slot) -> SlotOrder {
    let rank = super::vocabulary::ACTIONS
        .iter()
        .position(|action| action.id == slot.action && action.steps == slot.steps)
        .unwrap_or(usize::MAX);
    (
        rank,
        slot.deck.clone(),
        slot.slot.clone(),
        slot.param.clone(),
    )
}

/// The mapping being written for one device. Seeded from whatever mapping already
/// claims the port, so editing a shipped file and starting from nothing are one flow.
pub struct Draft {
    pub name: String,
    pub matches: Vec<String>,
    bindings: Vec<(Slot, Capture)>,
}

impl Draft {
    /// A port name carries the manufacturer's suffixes, so it claims itself
    /// verbatim until someone shortens it.
    pub fn empty(port: &str) -> Self {
        Self {
            name: port.to_string(),
            matches: vec![port.to_string()],
            bindings: Vec::new(),
        }
    }

    /// A mapping that plays whichever deck it is given names no deck on its bindings,
    /// but the panel maps one deck at a time, so they are read onto the first one and
    /// `scope` puts them back where they were.
    pub fn of(mapping: &Mapping) -> Self {
        Self {
            name: mapping.name.clone(),
            matches: mapping.matches.clone(),
            bindings: mapping
                .bindings
                .iter()
                .map(|spec| {
                    let mut slot = Slot::of(spec);
                    if slot.deck.is_none() && needs_deck(&slot) {
                        slot.deck = Some(FIRST_DECK.to_string());
                    }
                    (slot, Capture::of(spec))
                })
                .collect(),
        }
    }

    /// Replaces every address the slot had, so a slot the file reached twice cannot
    /// keep half of its old binding after an edit.
    pub fn bind(&mut self, slot: Slot, capture: Capture) {
        self.clear(&slot);
        self.bindings.push((slot, capture));
    }

    /// Every address the slot has, because a modifier's copy of a control is the same
    /// physical button and reports the same edges.
    pub fn set_button(&mut self, slot: &Slot, button: ButtonSpec) {
        for (held, capture) in &mut self.bindings {
            if held == slot && capture.note.is_some() {
                capture.button = Some(button);
            }
        }
    }

    pub fn clear(&mut self, slot: &Slot) {
        self.bindings.retain(|(held, _)| held != slot);
    }

    /// One entry per slot, in vocabulary order, so a row reads the same on every open
    /// and a slot the file reached twice shows both of its addresses.
    pub fn entries(&self) -> Vec<DraftBinding> {
        let mut entries: Vec<DraftBinding> = Vec::new();
        for (slot, capture) in &self.bindings {
            match entries.iter_mut().find(|entry| &entry.slot == slot) {
                Some(entry) => entry.captures.push(*capture),
                None => entries.push(DraftBinding {
                    slot: slot.clone(),
                    captures: vec![*capture],
                }),
            }
        }
        entries.sort_by_key(|entry| slot_order(&entry.slot));
        entries
    }

    pub fn specs(&self) -> Vec<BindingSpec> {
        self.bindings
            .iter()
            .map(|(slot, capture)| capture.into_spec(slot))
            .collect()
    }

    fn ordered_specs(&self) -> Vec<BindingSpec> {
        let mut specs = self.specs();
        specs.sort_by_key(|spec| slot_order(&Slot::of(spec)));
        specs
    }

    /// Read off the bindings rather than asked for: a surface with controls for one
    /// deck only ever gets bindings for one, and the user picks which deck it drives
    /// per device. Anything bound across decks names them itself.
    fn scope(&self) -> DeckScope {
        let mut decks: Vec<&String> = self
            .bindings
            .iter()
            .filter_map(|(slot, _)| slot.deck.as_ref())
            .collect();
        decks.sort();
        decks.dedup();
        if decks.len() > 1 {
            DeckScope::Fixed
        } else {
            DeckScope::Assigned
        }
    }

    /// Lowercase and hyphenated, the way the shipped files are named, so a folder
    /// of contributed mappings sorts and reads as one set.
    pub fn file_name(&self) -> String {
        let stem: String = self
            .name
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        let trimmed = stem
            .split('-')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        format!("{trimmed}.json")
    }

    /// The file as it would be contributed. Written flat: the deck template saves
    /// typing a file nobody types.
    pub fn file(&self) -> MappingFile {
        let decks = self.scope();
        let mut bindings = self.ordered_specs();
        if decks == DeckScope::Assigned {
            // A deck left on the binding would read as the one it happened to be
            // learned on, when the user picks it per device.
            for spec in &mut bindings {
                spec.deck = None;
            }
        }
        MappingFile {
            version: super::mapping::MAPPING_VERSION,
            name: self.name.clone(),
            matches: self.matches.clone(),
            decks,
            bindings,
        }
    }

    /// Slots sharing one address. A profile built from them is refused, so the UI
    /// has to show the collision rather than let it read as broken hardware.
    pub fn colliding(&self) -> Vec<Slot> {
        self.bindings
            .iter()
            .filter(|(slot, capture)| {
                self.bindings.iter().any(|(other, other_capture)| {
                    other != slot
                        && other_capture.channel == capture.channel
                        && other_capture.note == capture.note
                        && other_capture.cc == capture.cc
                })
            })
            .map(|(slot, _)| slot.clone())
            .collect()
    }
}

/// The slot a capture is being read for, and everything the surface has sent
/// since it was armed.
struct Armed {
    port: String,
    slot: Slot,
    kind: ControlKind,
    watch: Watch,
    last: Instant,
}

#[derive(Default)]
pub struct Learn {
    /// The port in learn mode. Its messages never reach the engine, so a capture
    /// cannot move a deck.
    port: Option<String>,
    armed: Option<Armed>,
    drafts: HashMap<String, Draft>,
}

/// What a settled capture reports back, so the UI names the slot it landed on
/// rather than assuming it is still the armed one.
pub struct Landed {
    pub port: String,
    pub slot: Slot,
    pub capture: Capture,
}

impl Learn {
    pub fn learning(&self, port: &str) -> bool {
        self.port.as_deref() == Some(port)
    }

    pub fn start(&mut self, port: &str, seed: Option<&Mapping>) {
        self.port = Some(port.to_string());
        self.armed = None;
        self.drafts
            .entry(port.to_string())
            .or_insert_with(|| seed.map_or_else(|| Draft::empty(port), Draft::of));
    }

    pub fn stop(&mut self) {
        self.port = None;
        self.armed = None;
    }

    pub fn draft(&self, port: &str) -> Option<&Draft> {
        self.drafts.get(port)
    }

    pub fn draft_mut(&mut self, port: &str) -> Option<&mut Draft> {
        self.drafts.get_mut(port)
    }

    pub fn discard(&mut self, port: &str) {
        self.drafts.remove(port);
        if self.learning(port) {
            self.armed = None;
        }
    }

    pub fn arm(&mut self, port: &str, slot: Slot) -> Result<(), String> {
        let kind = spec(&slot.action, slot.steps)
            .ok_or_else(|| format!("unknown action '{}'", slot.action))?
            .kind;
        self.armed = Some(Armed {
            port: port.to_string(),
            slot,
            kind,
            watch: Watch::default(),
            last: Instant::now(),
        });
        Ok(())
    }

    pub fn disarm(&mut self) {
        self.armed = None;
    }

    pub fn armed_slot(&self) -> Option<&Slot> {
        self.armed.as_ref().map(|armed| &armed.slot)
    }

    /// Returns the capture as it currently reads, so the UI shows a fader
    /// resolving while it is still being swept.
    pub fn observe(&mut self, port: &str, data: &[u8]) -> Option<(Slot, Capture)> {
        let armed = self.armed.as_mut().filter(|armed| armed.port == port)?;
        if !armed.watch.observe(data) {
            return None;
        }
        armed.last = Instant::now();
        let capture = infer(armed.kind, &armed.watch)?;
        Some((armed.slot.clone(), capture))
    }

    /// Called on a tick rather than per message, because the last message of a
    /// sweep looks like every other one until nothing follows it.
    pub fn settled(&mut self) -> Option<Landed> {
        let armed = self.armed.as_ref()?;
        if armed.watch.is_empty() || armed.last.elapsed() < SETTLE {
            return None;
        }
        let capture = infer(armed.kind, &armed.watch)?;
        let armed = self.armed.take()?;
        self.drafts
            .get_mut(&armed.port)?
            .bind(armed.slot.clone(), capture);
        Some(Landed {
            port: armed.port,
            slot: armed.slot,
            capture,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::wire::{CONTROL_CHANGE, NOTE_ON};
    use super::*;

    fn watched(kind: ControlKind, messages: &[Vec<u8>]) -> Option<Capture> {
        let mut watch = Watch::default();
        for data in messages {
            watch.observe(data);
        }
        infer(kind, &watch)
    }

    fn control_change(channel: u8, controller: u8, value: u8) -> Vec<u8> {
        vec![CONTROL_CHANGE | channel, controller, value]
    }

    fn sweep(channel: u8, controller: u8) -> Vec<Vec<u8>> {
        (0..=8)
            .map(|step| control_change(channel, controller, step * 15))
            .collect()
    }

    #[test]
    fn a_button_captures_its_note_on_the_channel_the_file_counts_from_one() {
        let captured =
            watched(ControlKind::Button, &[vec![NOTE_ON, 84, 127]]).expect("a note is a button");
        assert_eq!(
            captured,
            Capture {
                channel: 1,
                note: Some(84),
                cc: None,
                resolution: None,
                // No release arrived while the slot was armed.
                button: Some(ButtonSpec::Trigger)
            }
        );
    }

    #[test]
    fn a_button_released_while_armed_captures_as_momentary() {
        let captured = watched(
            ControlKind::Button,
            &[vec![NOTE_ON, 84, 127], vec![NOTE_ON, 84, 0]],
        )
        .expect("a press and its release");
        assert_eq!(captured.button, Some(ButtonSpec::Momentary));
    }

    // A release for a note this slot never saw pressed belongs to an earlier gesture.
    #[test]
    fn a_release_for_another_note_leaves_the_capture_a_trigger() {
        let captured = watched(
            ControlKind::Button,
            &[vec![NOTE_ON, 84, 127], vec![NOTE_ON, 11, 0]],
        )
        .expect("a press");
        assert_eq!(captured.button, Some(ButtonSpec::Trigger));
    }

    #[test]
    fn a_fader_sending_one_address_captures_seven_bit() {
        let captured =
            watched(ControlKind::Positional, &sweep(0, 7)).expect("a swept control change");
        assert_eq!(captured.cc, Some(7));
        assert_eq!(captured.resolution, Some(ResolutionSpec::SevenBit));
    }

    #[test]
    fn a_fader_sending_both_halves_captures_fourteen_bit_on_the_high_one() {
        let mut messages = sweep(0, 7);
        messages.extend(sweep(0, 39));
        let captured = watched(ControlKind::Positional, &messages).expect("both halves");
        assert_eq!(captured.cc, Some(7));
        assert_eq!(captured.resolution, Some(ResolutionSpec::FourteenBit));
    }

    // The low half moves on every step and the high half only on the ones that
    // carry, so the busiest address is the one the file must not name.
    #[test]
    fn a_high_half_that_moves_less_than_its_low_one_is_still_what_the_file_names() {
        let mut messages = sweep(0, 39);
        messages.push(control_change(0, 7, 64));
        let captured = watched(ControlKind::Positional, &messages).expect("both halves");
        assert_eq!(captured.cc, Some(7));
        assert_eq!(captured.resolution, Some(ResolutionSpec::FourteenBit));
    }

    #[test]
    fn a_platter_held_through_a_turn_captures_a_centre_delta() {
        let messages: Vec<Vec<u8>> = [65, 66, 67, 66, 65, 63, 62]
            .iter()
            .map(|value| control_change(0, 33, *value))
            .collect();
        let captured = watched(ControlKind::Relative, &messages).expect("a platter");
        assert_eq!(captured.resolution, Some(ResolutionSpec::CentreDelta));
    }

    #[test]
    fn a_browse_encoder_clicking_either_way_captures_a_signed_step() {
        let messages: Vec<Vec<u8>> = [1, 1, 127, 1, 127]
            .iter()
            .map(|value| control_change(6, 64, *value))
            .collect();
        let captured = watched(ControlKind::Relative, &messages).expect("an encoder");
        assert_eq!(captured.cc, Some(64));
        assert_eq!(captured.resolution, Some(ResolutionSpec::SignedStep));
    }

    #[test]
    fn a_button_press_on_a_slot_that_reads_a_fader_captures_nothing() {
        assert!(watched(ControlKind::Positional, &[vec![NOTE_ON, 84, 127]]).is_none());
    }

    #[test]
    fn a_fader_swept_into_a_slot_that_reads_a_button_captures_nothing() {
        assert!(watched(ControlKind::Button, &sweep(0, 7)).is_none());
    }

    #[test]
    fn a_note_on_a_channel_above_the_first_keeps_the_channel_it_arrived_on() {
        let captured = watched(ControlKind::Button, &[vec![NOTE_ON | 6, 65, 127]]).expect("a note");
        assert_eq!(captured.note, Some(65));
        assert_eq!(captured.channel, 7);
    }

    #[test]
    fn a_note_release_alone_never_settles_a_capture() {
        let mut learn = Learn::default();
        learn.start("port", None);
        learn.arm("port", play_slot()).expect("play_toggle");
        assert!(learn.observe("port", &[NOTE_ON, 11, 0]).is_none());
        assert!(learn.settled().is_none());
    }

    fn play_slot() -> Slot {
        Slot {
            action: "play_toggle".to_string(),
            deck: Some("A".to_string()),
            slot: None,
            param: None,
            steps: None,
        }
    }

    // The FLX6 reaches every deck's jog at two addresses, because SHIFT moves it.
    #[test]
    fn a_slot_the_file_reached_twice_is_one_entry_carrying_both_addresses() {
        let flx6 = super::super::mapping::built_in_mappings()
            .into_iter()
            .find(|mapping| mapping.name() == "DDJ-FLX6")
            .expect("the DDJ-FLX6 mapping");
        let draft = Draft::of(&flx6);
        let jog = draft
            .entries()
            .into_iter()
            .find(|entry| entry.slot.action == "jog" && entry.slot.deck.as_deref() == Some("A"))
            .expect("deck A's jog");
        let addresses: Vec<Option<u8>> = jog.captures.iter().map(|held| held.cc).collect();
        assert_eq!(addresses, vec![Some(33), Some(38)]);
    }

    #[test]
    fn rebinding_a_slot_replaces_every_address_it_had() {
        let flx6 = super::super::mapping::built_in_mappings()
            .into_iter()
            .find(|mapping| mapping.name() == "DDJ-FLX6")
            .expect("the DDJ-FLX6 mapping");
        let mut draft = Draft::of(&flx6);
        let jog = Slot {
            action: "jog".to_string(),
            deck: Some("A".to_string()),
            slot: None,
            param: None,
            steps: None,
        };
        draft.bind(
            jog.clone(),
            Capture {
                channel: 1,
                note: None,
                cc: Some(90),
                resolution: Some(ResolutionSpec::CentreDelta),
                button: None,
            },
        );
        let held: Vec<Option<u8>> = draft
            .specs()
            .iter()
            .filter(|spec| spec.action == "jog" && spec.deck.as_deref() == Some("A"))
            .map(|spec| spec.cc)
            .collect();
        assert_eq!(held, vec![Some(90)]);
    }

    #[test]
    fn rebinding_a_slot_replaces_its_capture_rather_than_adding_one() {
        let mut draft = Draft::empty("port");
        let first = Capture {
            channel: 1,
            note: Some(11),
            cc: None,
            resolution: None,
            button: Some(ButtonSpec::Momentary),
        };
        draft.bind(play_slot(), first);
        draft.bind(
            play_slot(),
            Capture {
                note: Some(12),
                ..first
            },
        );
        let specs = draft.specs();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].note, Some(12));
    }

    #[test]
    fn two_slots_on_one_address_are_both_reported_as_colliding() {
        let mut draft = Draft::empty("port");
        let taken = Capture {
            channel: 1,
            note: Some(11),
            cc: None,
            resolution: None,
            button: Some(ButtonSpec::Momentary),
        };
        let cue = Slot {
            action: "cue_toggle".to_string(),
            ..play_slot()
        };
        draft.bind(play_slot(), taken);
        assert!(draft.colliding().is_empty());
        draft.bind(cue.clone(), taken);
        assert_eq!(draft.colliding(), vec![play_slot(), cue]);
    }

    #[test]
    fn a_draft_seeded_from_a_shipped_mapping_reproduces_its_bindings() {
        let flx6 = super::super::mapping::built_in_mappings()
            .into_iter()
            .find(|mapping| mapping.name() == "DDJ-FLX6")
            .expect("the DDJ-FLX6 mapping");
        let draft = Draft::of(&flx6);
        let specs = draft.specs();
        assert_eq!(specs.len(), flx6.bindings.len());
        let jog = specs
            .iter()
            .find(|spec| spec.action == "jog" && spec.deck.as_deref() == Some("A"))
            .expect("deck A's jog");
        assert_eq!(jog.cc, Some(33));
        assert_eq!(jog.channel, 1);
    }

    #[test]
    fn a_browse_press_is_a_different_slot_from_the_one_the_other_way() {
        let forward = Slot {
            action: "browse".to_string(),
            deck: None,
            slot: None,
            param: None,
            steps: Some(1),
        };
        let back = Slot {
            steps: Some(-1),
            ..forward.clone()
        };
        let mut draft = Draft::empty("port");
        let capture = Capture {
            channel: 7,
            note: Some(65),
            cc: None,
            resolution: None,
            button: Some(ButtonSpec::Momentary),
        };
        draft.bind(forward, capture);
        draft.bind(back, capture);
        assert_eq!(draft.specs().len(), 2);
    }

    fn button(note: u8) -> Capture {
        Capture {
            channel: 1,
            note: Some(note),
            cc: None,
            resolution: None,
            button: Some(ButtonSpec::Momentary),
        }
    }

    fn play_on(deck: &str) -> Slot {
        Slot {
            deck: Some(deck.to_string()),
            ..play_slot()
        }
    }

    // Nobody is asked which kind of device this is: a surface with controls for one
    // deck only ever gets bindings for one, and the user picks its deck per device.
    #[test]
    fn a_surface_bound_on_one_deck_plays_whichever_deck_it_is_given() {
        let mut draft = Draft::empty("port");
        draft.bind(play_on("A"), button(11));
        assert_eq!(draft.file().decks, DeckScope::Assigned);
    }

    #[test]
    fn a_surface_bound_across_decks_names_the_decks_itself() {
        let mut draft = Draft::empty("port");
        draft.bind(play_on("A"), button(11));
        draft.bind(play_on("B"), button(12));
        assert_eq!(draft.file().decks, DeckScope::Fixed);
    }

    // A deck the file does not choose would read as the one it happened to be bound on.
    #[test]
    fn a_surface_that_plays_one_deck_writes_no_deck_on_its_bindings() {
        let mut draft = Draft::empty("port");
        draft.bind(play_on("A"), button(11));
        let written = draft.file();
        assert!(written.bindings.iter().all(|spec| spec.deck.is_none()));
    }

    #[test]
    fn a_surface_that_names_its_decks_keeps_them_on_every_binding() {
        let mut draft = Draft::empty("port");
        draft.bind(play_on("A"), button(11));
        draft.bind(play_on("B"), button(12));
        let written = draft.file();
        assert!(written.bindings.iter().all(|spec| spec.deck.is_some()));
    }

    // The shipped files are read and reviewed header first, and a contributed one
    // that sorts its keys diffs against them on every line.
    #[test]
    fn a_file_writes_its_keys_in_the_order_a_shipped_mapping_uses() {
        let draft = Draft::empty("port");
        let written = serde_json::to_string(&draft.file()).expect("the file");
        assert_eq!(
            written,
            r#"{"version":3,"name":"port","match":["port"],"decks":"assigned","bindings":[]}"#
        );
    }

    #[test]
    fn a_file_writes_its_bindings_in_vocabulary_order_not_capture_order() {
        let mut draft = Draft::empty("port");
        let note = Capture {
            channel: 7,
            note: Some(65),
            cc: None,
            resolution: None,
            button: Some(ButtonSpec::Momentary),
        };
        let browse = |steps| Slot {
            action: "browse".to_string(),
            deck: None,
            slot: None,
            param: None,
            steps,
        };
        draft.bind(browse(Some(1)), note);
        draft.bind(
            Slot {
                action: "enter".to_string(),
                ..browse(None)
            },
            note,
        );
        draft.bind(browse(Some(-1)), note);

        let written: Vec<(String, Option<i32>)> = draft
            .file()
            .bindings
            .into_iter()
            .map(|binding| (binding.action, binding.steps))
            .collect();
        assert_eq!(
            written,
            vec![
                ("browse".to_string(), Some(-1)),
                ("browse".to_string(), Some(1)),
                ("enter".to_string(), None)
            ]
        );
    }

    #[test]
    fn a_browse_encoder_and_a_browse_press_read_different_controls() {
        let mut learn = Learn::default();
        learn.start("port", None);
        let encoder = Slot {
            action: "browse".to_string(),
            deck: None,
            slot: None,
            param: None,
            steps: None,
        };
        learn.arm("port", encoder).expect("browse");
        assert!(learn.observe("port", &[NOTE_ON | 6, 65, 127]).is_none());

        let press = Slot {
            action: "browse".to_string(),
            deck: None,
            slot: None,
            param: None,
            steps: Some(1),
        };
        learn.arm("port", press).expect("browse forward");
        assert!(learn.observe("port", &[NOTE_ON | 6, 65, 127]).is_some());
    }

    #[test]
    fn a_port_name_becomes_one_hyphenated_file_name() {
        let mut draft = Draft::empty("DDJ-FLX6 MIDI 1");
        assert_eq!(draft.file_name(), "ddj-flx6-midi-1.json");
        draft.name = "XDJ-1000MK2".to_string();
        assert_eq!(draft.file_name(), "xdj-1000mk2.json");
    }

    #[test]
    fn messages_for_a_port_that_is_not_the_armed_one_are_ignored() {
        let mut learn = Learn::default();
        learn.start("port", None);
        learn.arm("port", play_slot()).expect("play_toggle");
        assert!(learn.observe("other", &[NOTE_ON, 11, 127]).is_none());
    }

    #[test]
    fn arming_an_action_the_vocabulary_does_not_offer_is_refused() {
        let mut learn = Learn::default();
        learn.start("port", None);
        assert!(learn
            .arm(
                "port",
                Slot {
                    action: "teleport".to_string(),
                    deck: None,
                    slot: None,
                    param: None,
                    steps: None,
                },
            )
            .is_err());
    }
}
