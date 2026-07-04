// Derives a track-list from a recorded session: the first moment each loaded
// track becomes audible in the mix (playing AND fader above silence), in
// recording-elapsed time. This is the data a CUE sheet needs. Pure event
// processing; tag lookup and CUE text formatting stay in the audio crate.

use crate::event::{SessionCommand, SessionEvent};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct CuePoint {
    pub elapsed_ms: f64,
    pub track_path: String,
}

struct DeckAudible {
    loaded_path: Option<String>,
    is_playing: bool,
    gain: f32,
    // Whether a cue point was already emitted for the current loaded instance.
    // Reset on (re)load/eject so a fresh load can emit again, but a track that is
    // faded out and brought back in keeps its first audible time.
    recorded: bool,
}

impl Default for DeckAudible {
    fn default() -> Self {
        // The mixer fader rests at unity, so a deck is audible the moment it
        // plays unless a set_volume event says otherwise.
        Self {
            loaded_path: None,
            is_playing: false,
            gain: 1.0,
            recorded: false,
        }
    }
}

pub fn build_cue_points(events: &[SessionEvent]) -> Vec<CuePoint> {
    let mut decks: HashMap<String, DeckAudible> = HashMap::new();
    let mut points = Vec::new();

    for event in events {
        let Some(command) = event.command() else {
            continue;
        };
        match command {
            // Headphone cue previews (CuePreviewStart/End) are deliberately not
            // treated as playing: previewing a track is not bringing it into the
            // mix.
            SessionCommand::LoadTrack { deck, path, .. } => {
                let state = decks.entry(deck.to_string()).or_default();
                state.loaded_path = Some(path.to_string());
                state.is_playing = false;
                state.recorded = false;
            }
            SessionCommand::DeckSnapshot {
                deck,
                path,
                is_playing,
                ..
            } => {
                let state = decks.entry(deck.to_string()).or_default();
                state.loaded_path = Some(path.to_string());
                state.is_playing = is_playing;
                // The snapshot carries the strip gain at record start (gain is
                // not part of the DeckSnapshot command, so read it from the raw
                // event); absent in older logs, where unity is the safe default.
                if let Some(gain) = event.gain {
                    state.gain = gain;
                }
                state.recorded = false;
            }
            SessionCommand::EjectTrack { deck } => {
                let state = decks.entry(deck.to_string()).or_default();
                state.loaded_path = None;
                state.is_playing = false;
                state.recorded = false;
            }
            SessionCommand::Play { deck, .. } => {
                decks.entry(deck.to_string()).or_default().is_playing = true;
            }
            SessionCommand::Stop { deck } | SessionCommand::StopAtCue { deck, .. } => {
                decks.entry(deck.to_string()).or_default().is_playing = false;
            }
            SessionCommand::SetVolume { deck, gain } => {
                decks.entry(deck.to_string()).or_default().gain = gain;
            }
            _ => {}
        }

        let Some(deck_id) = command.deck_id() else {
            continue;
        };
        let Some(state) = decks.get_mut(deck_id) else {
            continue;
        };
        if !state.recorded && state.is_playing && state.gain > 0.0 {
            if let Some(path) = &state.loaded_path {
                points.push(CuePoint {
                    elapsed_ms: event.elapsed_ms,
                    track_path: path.clone(),
                });
                state.recorded = true;
            }
        }
    }

    points
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(elapsed_ms: f64, event_type: &str, deck: &str) -> SessionEvent {
        SessionEvent::at(elapsed_ms, event_type, deck)
    }

    #[test]
    fn emits_first_audible_time_per_track() {
        let events = vec![
            SessionEvent {
                path: Some("/a.wav".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            SessionEvent {
                path: Some("/b.wav".to_string()),
                ..ev(2000.0, "load_track", "B")
            },
            ev(3000.0, "play", "B"),
        ];
        let points = build_cue_points(&events);
        assert_eq!(
            points,
            vec![
                CuePoint {
                    elapsed_ms: 1000.0,
                    track_path: "/a.wav".to_string()
                },
                CuePoint {
                    elapsed_ms: 3000.0,
                    track_path: "/b.wav".to_string()
                },
            ]
        );
    }

    #[test]
    fn fader_down_delays_audible_time() {
        let events = vec![
            SessionEvent {
                gain: Some(0.0),
                ..ev(0.0, "set_volume", "A")
            },
            SessionEvent {
                path: Some("/a.wav".to_string()),
                ..ev(100.0, "load_track", "A")
            },
            ev(200.0, "play", "A"),
            SessionEvent {
                gain: Some(0.8),
                ..ev(5000.0, "set_volume", "A")
            },
        ];
        let points = build_cue_points(&events);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].elapsed_ms, 5000.0);
    }

    #[test]
    fn snapshot_gain_decides_audibility_at_record_start() {
        // A deck already playing at record start with the fader down is not
        // audible until it is brought up.
        let events = vec![
            SessionEvent {
                path: Some("/a.wav".to_string()),
                is_playing: Some(true),
                gain: Some(0.0),
                ..ev(0.0, "deck_snapshot", "A")
            },
            SessionEvent {
                gain: Some(1.0),
                ..ev(8000.0, "set_volume", "A")
            },
        ];
        let points = build_cue_points(&events);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].elapsed_ms, 8000.0);
    }

    #[test]
    fn snapshot_playing_and_audible_emits_at_start() {
        let events = vec![SessionEvent {
            path: Some("/a.wav".to_string()),
            is_playing: Some(true),
            gain: Some(1.0),
            ..ev(0.0, "deck_snapshot", "A")
        }];
        let points = build_cue_points(&events);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].elapsed_ms, 0.0);
    }

    #[test]
    fn out_then_back_in_keeps_first_time() {
        let events = vec![
            SessionEvent {
                path: Some("/a.wav".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(100.0, "play", "A"),
            SessionEvent {
                gain: Some(0.0),
                ..ev(2000.0, "set_volume", "A")
            },
            SessionEvent {
                gain: Some(1.0),
                ..ev(4000.0, "set_volume", "A")
            },
        ];
        let points = build_cue_points(&events);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].elapsed_ms, 100.0);
    }
}
