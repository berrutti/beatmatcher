use crate::event::{SessionCommand, SessionEvent};
use crate::param::is_fader_gain;
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
    xfader_assign: crate::XfaderAssign,
    // Whether a cue point was already emitted for the current loaded instance.
    // Reset on (re)load/eject so a fresh load can emit again, but a track that is
    // faded out and brought back in keeps its first audible time.
    recorded: bool,
}

impl Default for DeckAudible {
    fn default() -> Self {
        // The mixer fader rests at unity, so a deck is audible the moment it
        // plays unless a fader/gain event says otherwise.
        Self {
            loaded_path: None,
            is_playing: false,
            gain: 1.0,
            xfader_assign: crate::XfaderAssign::Thru,
            recorded: false,
        }
    }
}

impl DeckAudible {
    fn audible(&self, xfader_position: f64) -> bool {
        self.is_playing && self.gain > 0.0 && self.xfader_assign.gain(xfader_position) > 0.0
    }
}

pub fn build_cue_points(events: &[SessionEvent]) -> Vec<CuePoint> {
    let mut decks: HashMap<String, DeckAudible> = HashMap::new();
    let mut points = Vec::new();
    let mut xfader_position = 0.0;

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
            SessionCommand::SetParam {
                deck: Some(deck),
                slot,
                param,
                value,
                ..
            } if is_fader_gain(slot, param) => {
                decks.entry(deck.to_string()).or_default().gain = value as f32;
            }
            SessionCommand::SetXfaderAssign { deck, assign } => {
                decks.entry(deck.to_string()).or_default().xfader_assign = assign;
            }
            SessionCommand::SetParam {
                deck: None,
                slot: "xfader",
                param: "position",
                value,
                ..
            } => {
                xfader_position = value;
            }
            _ => {}
        }

        // A master crossfader move names no deck and can open any of them, so it re-checks
        // all. Sorted because an unordered HashMap walk renumbered tracks between runs.
        let moved: Vec<String> = match command.deck_id() {
            Some(deck_id) => vec![deck_id.to_string()],
            None => {
                let mut all: Vec<String> = decks.keys().cloned().collect();
                all.sort();
                all
            }
        };
        for deck_id in moved {
            let Some(state) = decks.get_mut(&deck_id) else {
                continue;
            };
            if !state.recorded && state.audible(xfader_position) {
                if let Some(path) = &state.loaded_path {
                    points.push(CuePoint {
                        elapsed_ms: event.elapsed_ms,
                        track_path: path.clone(),
                    });
                    state.recorded = true;
                }
            }
        }
    }

    points
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(elapsed_ms: f64, event_type: &str, deck: &str) -> SessionEvent {
        SessionEvent::at(elapsed_ms, event_type, deck)
    }

    #[test]
    fn emits_first_audible_time_per_track() {
        let events = vec![
            SessionEvent {
                path: Some("/a.wav".to_string()),
                ..make_event(0.0, "load_track", "A")
            },
            make_event(1000.0, "play", "A"),
            SessionEvent {
                path: Some("/b.wav".to_string()),
                ..make_event(2000.0, "load_track", "B")
            },
            make_event(3000.0, "play", "B"),
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
            SessionEvent::param(0.0, Some("A"), "fader", "gain", 0.0),
            SessionEvent {
                path: Some("/a.wav".to_string()),
                ..make_event(100.0, "load_track", "A")
            },
            make_event(200.0, "play", "A"),
            SessionEvent::param(5000.0, Some("A"), "fader", "gain", 0.8),
        ];
        let points = build_cue_points(&events);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].elapsed_ms, 5000.0);
    }

    #[test]
    fn decks_opened_by_one_crossfader_move_are_listed_in_a_stable_order() {
        let events = vec![
            assign_event(0.0, "A", "b"),
            assign_event(0.0, "B", "b"),
            assign_event(0.0, "C", "b"),
            assign_event(0.0, "D", "b"),
            SessionEvent::param(0.0, None, "xfader", "position", -1.0),
            SessionEvent {
                path: Some("/a.wav".to_string()),
                ..make_event(100.0, "load_track", "A")
            },
            SessionEvent {
                path: Some("/b.wav".to_string()),
                ..make_event(100.0, "load_track", "B")
            },
            SessionEvent {
                path: Some("/c.wav".to_string()),
                ..make_event(100.0, "load_track", "C")
            },
            SessionEvent {
                path: Some("/d.wav".to_string()),
                ..make_event(100.0, "load_track", "D")
            },
            make_event(200.0, "play", "A"),
            make_event(200.0, "play", "B"),
            make_event(200.0, "play", "C"),
            make_event(200.0, "play", "D"),
            SessionEvent::param(5000.0, None, "xfader", "position", 1.0),
        ];

        let expected: Vec<String> = build_cue_points(&events)
            .into_iter()
            .map(|point| point.track_path)
            .collect();
        assert_eq!(expected, vec!["/a.wav", "/b.wav", "/c.wav", "/d.wav"]);

        for _ in 0..200 {
            let paths: Vec<String> = build_cue_points(&events)
                .into_iter()
                .map(|point| point.track_path)
                .collect();
            assert_eq!(paths, expected);
        }
    }

    fn assign_event(elapsed_ms: f64, deck: &str, assign: &str) -> SessionEvent {
        SessionEvent {
            assign: Some(assign.to_string()),
            ..make_event(elapsed_ms, "set_xfader_assign", deck)
        }
    }

    #[test]
    fn a_deck_crossfaded_away_is_not_audible_yet() {
        let events = vec![
            assign_event(0.0, "A", "a"),
            SessionEvent::param(0.0, None, "xfader", "position", 1.0),
            SessionEvent {
                path: Some("/a.wav".to_string()),
                ..make_event(100.0, "load_track", "A")
            },
            make_event(200.0, "play", "A"),
            SessionEvent::param(5000.0, None, "xfader", "position", -1.0),
        ];
        let points = build_cue_points(&events);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].elapsed_ms, 5000.0);
    }

    #[test]
    fn bringing_the_crossfader_back_records_without_a_deck_event() {
        let events = vec![
            assign_event(0.0, "B", "b"),
            SessionEvent::param(0.0, None, "xfader", "position", -1.0),
            SessionEvent {
                path: Some("/b.wav".to_string()),
                ..make_event(100.0, "load_track", "B")
            },
            make_event(200.0, "play", "B"),
            SessionEvent::param(4000.0, None, "xfader", "position", 1.0),
        ];
        let points = build_cue_points(&events);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].track_path, "/b.wav");
        assert_eq!(points[0].elapsed_ms, 4000.0);
    }

    #[test]
    fn a_thru_deck_ignores_the_crossfader() {
        let events = vec![
            SessionEvent::param(0.0, None, "xfader", "position", 1.0),
            SessionEvent {
                path: Some("/a.wav".to_string()),
                ..make_event(100.0, "load_track", "A")
            },
            make_event(200.0, "play", "A"),
        ];
        let points = build_cue_points(&events);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].elapsed_ms, 200.0);
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
                ..make_event(0.0, "deck_snapshot", "A")
            },
            SessionEvent::param(8000.0, Some("A"), "fader", "gain", 1.0),
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
            ..make_event(0.0, "deck_snapshot", "A")
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
                ..make_event(0.0, "load_track", "A")
            },
            make_event(100.0, "play", "A"),
            SessionEvent::param(2000.0, Some("A"), "fader", "gain", 0.0),
            SessionEvent::param(4000.0, Some("A"), "fader", "gain", 1.0),
        ];
        let points = build_cue_points(&events);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].elapsed_ms, 100.0);
    }
}
