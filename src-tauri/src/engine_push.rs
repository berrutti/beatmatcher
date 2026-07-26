use crate::audio::AppAudio;
use crate::commands::DeckSyncPayload;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Emitter;

const FLUSH_MS: u64 = 33;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ParamOrigin {
    Ui,
    Midi,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamChange {
    deck: String,
    slot: String,
    param: String,
    value: f32,
}

/// Transport does not reach the UI as a param either, and it carries more than
/// one number, so it rides its own event rather than being flattened into
/// `ParamChange`.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TransportChange {
    deck: String,
    #[serde(flatten)]
    state: DeckSyncPayload,
}

/// Rate rides its own event because the frontend derives the displayed bpm and
/// pitch offset from it, rather than storing it as the engine does.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RateChange {
    deck: String,
    rate: f64,
}

// Cue is not a manifest param, so it cannot be addressed by slot and param the
// way the rest can. It still reaches the UI on this channel, under a slot name
// the mixer store dispatches on.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Address {
    Param(String, String, String),
    Cue(String),
    Xfader,
    XfaderAssign(String),
    Transport(String),
    Rate(String),
}

/// One lock over both, so a mark cannot land its cleared flag in the batch that
/// its address misses.
#[derive(Default)]
struct Pending {
    dirty: BTreeSet<Address>,
    loops_cleared: BTreeSet<String>,
}

pub struct EnginePush {
    pending: Mutex<Pending>,
}

impl EnginePush {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(Pending::default()),
        }
    }

    /// A UI write is the UI's own value on its way back, so sending it invites
    /// the control under the pointer to jump to a value it has already left.
    fn insert(&self, origin: ParamOrigin, address: Address) {
        if origin == ParamOrigin::Ui {
            return;
        }
        self.pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .dirty
            .insert(address);
    }

    pub(crate) fn mark(&self, origin: ParamOrigin, deck: &str, slot: &str, param: &str) {
        self.insert(
            origin,
            Address::Param(deck.to_string(), slot.to_string(), param.to_string()),
        );
    }

    pub(crate) fn mark_cue(&self, origin: ParamOrigin, deck: &str) {
        self.insert(origin, Address::Cue(deck.to_string()));
    }

    pub(crate) fn mark_xfader(&self, origin: ParamOrigin) {
        self.insert(origin, Address::Xfader);
    }

    pub(crate) fn mark_xfader_assign(&self, origin: ParamOrigin, deck: &str) {
        self.insert(origin, Address::XfaderAssign(deck.to_string()));
    }

    /// Whether the loop region was destroyed is a fact about the press rather
    /// than about the deck's current state, so it is the one thing here that
    /// cannot be read at flush and has to be carried.
    pub(crate) fn mark_transport(
        &self,
        origin: ParamOrigin,
        deck: &str,
        loop_region_cleared: bool,
    ) {
        if loop_region_cleared && origin != ParamOrigin::Ui {
            self.pending
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .loops_cleared
                .insert(deck.to_string());
        }
        self.insert(origin, Address::Transport(deck.to_string()));
    }

    pub(crate) fn mark_rate(&self, origin: ParamOrigin, deck: &str) {
        self.insert(origin, Address::Rate(deck.to_string()));
    }

    fn take(&self) -> Pending {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        std::mem::take(&mut *pending)
    }
}

/// Display only: the engine applied each value when it was written, so a flush
/// period of latency costs a redraw and nothing else.
///
/// Values are read here rather than captured at write time, so a push can never
/// carry a value that a later write has already replaced.
pub fn start(app: tauri::AppHandle, audio: Arc<AppAudio>, push: Arc<EnginePush>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(FLUSH_MS));
        let pending = push.take();
        let loops_cleared = pending.loops_cleared;
        let mut transport: Vec<TransportChange> = Vec::new();
        let mut rates: Vec<RateChange> = Vec::new();
        let batch: Vec<ParamChange> = pending
            .dirty
            .into_iter()
            .filter_map(|address| match address {
                Address::Transport(deck) => {
                    let state = DeckSyncPayload::from_deck(
                        &audio
                            .deck(&deck)?
                            .lock()
                            .unwrap_or_else(|error| error.into_inner()),
                        loops_cleared.contains(&deck),
                    );
                    transport.push(TransportChange { deck, state });
                    None
                }
                Address::Rate(deck) => {
                    let rate = audio
                        .deck(&deck)?
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .playback_rate;
                    rates.push(RateChange { deck, rate });
                    None
                }
                Address::Param(deck, slot, param) => {
                    let value = audio
                        .strip(&deck)?
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .param(&slot, &param)?;
                    Some(ParamChange {
                        deck,
                        slot,
                        param,
                        value,
                    })
                }
                Address::Cue(deck) => {
                    let active = audio
                        .strip(&deck)?
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .cue_active;
                    Some(ParamChange {
                        deck,
                        slot: "cue".to_string(),
                        param: "active".to_string(),
                        value: if active { 1.0 } else { 0.0 },
                    })
                }
                // Master scope has no deck, and the store dispatches on the slot,
                // so an empty deck is what distinguishes it from the assign below.
                Address::Xfader => Some(ParamChange {
                    deck: String::new(),
                    slot: "xfader".to_string(),
                    param: "position".to_string(),
                    value: audio.monitor.xfader_position(),
                }),
                Address::XfaderAssign(deck) => {
                    let assign = audio
                        .strip(&deck)?
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .xfader_assign;
                    Some(ParamChange {
                        deck,
                        slot: "xfader".to_string(),
                        param: "assign".to_string(),
                        value: match assign {
                            session_core::XfaderAssign::Thru => 0.0,
                            session_core::XfaderAssign::A => 1.0,
                            session_core::XfaderAssign::B => 2.0,
                        },
                    })
                }
            })
            .collect();
        if !batch.is_empty() {
            app.emit("engine-params", batch).ok();
        }
        if !transport.is_empty() {
            app.emit("engine-transport", transport).ok();
        }
        if !rates.is_empty() {
            app.emit("engine-rate", rates).ok();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ui_write_is_never_pushed_back() {
        let push = EnginePush::new();
        push.mark(ParamOrigin::Ui, "A", "eq", "low");
        assert!(push.take().dirty.is_empty());
    }

    #[test]
    fn a_write_the_ui_did_not_make_is_pushed() {
        let push = EnginePush::new();
        push.mark(ParamOrigin::Midi, "A", "eq", "low");
        assert_eq!(push.take().dirty.len(), 1);
    }

    // What bounds the channel: a controller sweeping a knob during one flush
    // period costs one message, not one per tick.
    #[test]
    fn repeated_writes_to_one_address_collapse() {
        let push = EnginePush::new();
        for _ in 0..500 {
            push.mark(ParamOrigin::Midi, "A", "eq", "low");
        }
        push.mark(ParamOrigin::Midi, "A", "eq", "mid");
        push.mark(ParamOrigin::Midi, "B", "eq", "low");

        let taken = push.take();
        assert_eq!(taken.dirty.len(), 3);
    }

    // The crossfader rides the same channel but at master scope, and its assign
    // is per deck, so the two must not collapse into each other.
    #[test]
    fn the_crossfader_and_its_assigns_are_separate_addresses() {
        let push = EnginePush::new();
        push.mark_xfader(ParamOrigin::Midi);
        push.mark_xfader(ParamOrigin::Midi);
        push.mark_xfader_assign(ParamOrigin::Midi, "A");
        push.mark_xfader_assign(ParamOrigin::Midi, "B");

        assert_eq!(push.take().dirty.len(), 3);
    }

    #[test]
    fn a_ui_crossfader_move_is_never_pushed_back() {
        let push = EnginePush::new();
        push.mark_xfader(ParamOrigin::Ui);
        push.mark_xfader_assign(ParamOrigin::Ui, "A");
        assert!(push.take().dirty.is_empty());
    }

    #[test]
    fn a_ui_transport_move_is_never_pushed_back() {
        let push = EnginePush::new();
        push.mark_transport(ParamOrigin::Ui, "A", true);
        let taken = push.take();
        assert!(taken.dirty.is_empty());
        assert!(taken.loops_cleared.is_empty());
    }

    // Playing state and position are re-read at flush, but a destroyed loop
    // region is not readable there, so it has to survive the collapse of every
    // other press in the window.
    #[test]
    fn a_cleared_loop_region_survives_repeated_transport_marks() {
        let push = EnginePush::new();
        push.mark_transport(ParamOrigin::Midi, "A", true);
        push.mark_transport(ParamOrigin::Midi, "A", false);
        push.mark_transport(ParamOrigin::Midi, "B", false);

        let taken = push.take();
        assert_eq!(taken.dirty.len(), 2);
        assert!(taken.loops_cleared.contains("A"));
        assert!(!taken.loops_cleared.contains("B"));
    }

    // A flag left behind would clear a region the deck acquired after the press
    // that destroyed the previous one.
    #[test]
    fn a_cleared_loop_region_does_not_outlive_its_batch() {
        let push = EnginePush::new();
        push.mark_transport(ParamOrigin::Midi, "A", true);
        push.take();
        push.mark_transport(ParamOrigin::Midi, "A", false);

        assert!(push.take().loops_cleared.is_empty());
    }

    // Transport is a deck's own address, so it must not collapse into the param
    // addresses that share its deck.
    #[test]
    fn transport_is_a_separate_address_from_a_param() {
        let push = EnginePush::new();
        push.mark_transport(ParamOrigin::Midi, "A", false);
        push.mark(ParamOrigin::Midi, "A", "fader", "gain");
        push.mark_cue(ParamOrigin::Midi, "A");
        push.mark_rate(ParamOrigin::Midi, "A");

        assert_eq!(push.take().dirty.len(), 4);
    }

    // What bounds a tempo fader sweep: 14 bits of travel is thousands of writes,
    // and the UI only needs where it came to rest.
    #[test]
    fn a_tempo_sweep_collapses_to_one_rate_per_deck() {
        let push = EnginePush::new();
        for _ in 0..2000 {
            push.mark_rate(ParamOrigin::Midi, "A");
        }
        push.mark_rate(ParamOrigin::Midi, "B");

        assert_eq!(push.take().dirty.len(), 2);
    }

    #[test]
    fn a_ui_rate_move_is_never_pushed_back() {
        let push = EnginePush::new();
        push.mark_rate(ParamOrigin::Ui, "A");
        assert!(push.take().dirty.is_empty());
    }

    #[test]
    fn taking_drains() {
        let push = EnginePush::new();
        push.mark(ParamOrigin::Midi, "A", "eq", "low");
        assert_eq!(push.take().dirty.len(), 1);
        assert!(push.take().dirty.is_empty());
    }
}
