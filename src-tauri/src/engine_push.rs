use crate::audio::AppAudio;
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

// Cue is not a manifest param, so it cannot be addressed by slot and param the
// way the rest can. It still reaches the UI on this channel, under a slot name
// the mixer store dispatches on.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Address {
    Param(String, String, String),
    Cue(String),
}

pub struct EnginePush {
    dirty: Mutex<BTreeSet<Address>>,
}

impl EnginePush {
    pub fn new() -> Self {
        Self {
            dirty: Mutex::new(BTreeSet::new()),
        }
    }

    /// A UI write is the UI's own value on its way back, so sending it invites
    /// the control under the pointer to jump to a value it has already left.
    fn insert(&self, origin: ParamOrigin, address: Address) {
        if origin == ParamOrigin::Ui {
            return;
        }
        self.dirty
            .lock()
            .unwrap_or_else(|error| error.into_inner())
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

    fn take(&self) -> Vec<Address> {
        let mut dirty = self.dirty.lock().unwrap_or_else(|error| error.into_inner());
        std::mem::take(&mut *dirty).into_iter().collect()
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
        let batch: Vec<ParamChange> = push
            .take()
            .into_iter()
            .filter_map(|address| match address {
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
            })
            .collect();
        if !batch.is_empty() {
            app.emit("engine-params", batch).ok();
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
        assert!(push.take().is_empty());
    }

    #[test]
    fn a_write_the_ui_did_not_make_is_pushed() {
        let push = EnginePush::new();
        push.mark(ParamOrigin::Midi, "A", "eq", "low");
        assert_eq!(push.take().len(), 1);
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
        assert_eq!(taken.len(), 3);
    }

    #[test]
    fn taking_drains() {
        let push = EnginePush::new();
        push.mark(ParamOrigin::Midi, "A", "eq", "low");
        assert_eq!(push.take().len(), 1);
        assert!(push.take().is_empty());
    }
}
