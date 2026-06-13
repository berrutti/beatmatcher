use serde::Deserialize;

#[derive(Deserialize)]
pub struct Event {
    #[serde(rename = "type")]
    pub kind: String,
    pub elapsed_ms: f64,
    pub deck: Option<String>,
    pub is_playing: Option<bool>,
}

fn event_order(a: &Event, b: &Event) -> std::cmp::Ordering {
    let bucket = |e: &Event| e.elapsed_ms.round() as i64;
    let snapshot_rank = |e: &Event| u8::from(e.kind != "deck_snapshot");
    bucket(a)
        .cmp(&bucket(b))
        .then_with(|| snapshot_rank(a).cmp(&snapshot_rank(b)))
        .then_with(|| {
            a.elapsed_ms
                .partial_cmp(&b.elapsed_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

pub fn deck_is_playing_at(events: &[Event], deck: &str, from_ms: f64) -> bool {
    let mut ordered: Vec<&Event> = events
        .iter()
        .filter(|e| e.deck.as_deref() == Some(deck) && e.elapsed_ms <= from_ms)
        .collect();
    ordered.sort_by(|a, b| event_order(a, b));

    let mut playing = false;
    for event in ordered {
        match event.kind.as_str() {
            "deck_snapshot" => playing = event.is_playing.unwrap_or(false),
            "load_track" => playing = false,
            "play" => playing = true,
            "stop" => playing = false,
            _ => {}
        }
    }
    playing
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub fn deck_is_playing_at(events_json: &str, deck: &str, from_ms: f64) -> bool {
        let events: Vec<super::Event> = serde_json::from_str(events_json).unwrap_or_default();
        super::deck_is_playing_at(&events, deck, from_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: &str, ms: f64, is_playing: Option<bool>) -> Event {
        Event {
            kind: kind.to_string(),
            elapsed_ms: ms,
            deck: Some("A".to_string()),
            is_playing,
        }
    }

    #[test]
    fn deck_snapshot_landing_after_a_play_does_not_silence_the_deck() {
        // The mix.bms order, with the snapshot a sub-ms hair after the play.
        let events = vec![
            ev("load_track", 0.025207999999992126, None),
            ev("play", 0.025207999999992126, None),
            ev("deck_snapshot", 0.025208, Some(false)),
            ev("stop", 3302.0, None),
        ];
        assert!(deck_is_playing_at(&events, "A", 2000.0));
        assert!(!deck_is_playing_at(&events, "A", 4000.0));
    }
}
