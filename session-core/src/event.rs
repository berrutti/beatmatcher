// The session event model: the raw deserialized .bms event and its typed
// command form. The three interpreters (scrub simulation, live playback,
// offline render) all match exhaustively on `SessionCommand`, so adding a
// variant forces a compile error in each until its behavior is decided.

// Serializes back to the same shape the frontend writes to .bms: only the
// fields actually set appear (skip_serializing_if), so edit ops that synthesize
// events round-trip to clean JSON without a wall of nulls.
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct SessionEvent {
    pub elapsed_ms: f64,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deck: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sec: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat_offset_sec: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_sec: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_sec: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_playing: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_sec: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cue_point_sec: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_active: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_end_sec: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playback_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cue_sec: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer_size_frames: Option<u32>,
}

impl SessionEvent {
    // Builds a per-deck event with only `elapsed_ms`, `event_type`, and `deck`
    // set; callers fill in the remaining fields via struct-update syntax.
    pub fn at(elapsed_ms: f64, event_type: &str, deck: &str) -> SessionEvent {
        SessionEvent {
            elapsed_ms,
            event_type: event_type.to_string(),
            deck: Some(deck.to_string()),
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct SessionFile {
    pub events: Vec<SessionEvent>,
}

// Every replayable command, with the fields each one actually consumes.
// Borrowed from the raw event, so conversion never allocates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SessionCommand<'a> {
    DeckSnapshot {
        deck: &'a str,
        path: &'a str,
        position_sec: Option<f64>,
        cue_point_sec: Option<f64>,
        bpm: Option<f64>,
        playback_rate: Option<f64>,
        loop_active: Option<bool>,
        loop_end_sec: Option<f64>,
        is_playing: bool,
    },
    LoadTrack {
        deck: &'a str,
        path: &'a str,
        beat_offset_sec: Option<f64>,
    },
    EjectTrack {
        deck: &'a str,
    },
    Play {
        deck: &'a str,
        sec: Option<f64>,
    },
    Stop {
        deck: &'a str,
    },
    StopAtCue {
        deck: &'a str,
        cue_point_sec: Option<f64>,
    },
    Seek {
        deck: &'a str,
        sec: f64,
    },
    SetVolume {
        deck: &'a str,
        gain: f32,
    },
    SetEq {
        deck: &'a str,
        band: &'a str,
        db: f32,
    },
    SetFilter {
        deck: &'a str,
        value: f32,
    },
    SetFilterActive {
        deck: &'a str,
        active: bool,
    },
    SetPlaybackRate {
        deck: &'a str,
        rate: f64,
    },
    SetNudge {
        deck: &'a str,
        percent: f64,
    },
    SetMasterGain {
        gain: f32,
    },
    SetBeatGrid {
        deck: &'a str,
        bpm: Option<f64>,
        beat_offset_sec: Option<f64>,
    },
    LoopIn {
        deck: &'a str,
        cue_sec: Option<f64>,
    },
    LoopOut {
        deck: &'a str,
        start_sec: Option<f64>,
        end_sec: Option<f64>,
    },
    ExitLoop {
        deck: &'a str,
    },
    Reloop {
        deck: &'a str,
    },
    CuePreviewStart {
        deck: &'a str,
        cue_point_sec: Option<f64>,
    },
    CuePreviewEnd {
        deck: &'a str,
        cue_point_sec: Option<f64>,
    },
}

impl<'a> SessionCommand<'a> {
    // The deck a command targets, or None for commands that act on the master strip.
    pub fn deck_id(&self) -> Option<&'a str> {
        use SessionCommand::*;
        match *self {
            SetMasterGain { .. } => None,
            DeckSnapshot { deck, .. }
            | LoadTrack { deck, .. }
            | EjectTrack { deck }
            | Play { deck, .. }
            | Stop { deck }
            | StopAtCue { deck, .. }
            | Seek { deck, .. }
            | SetVolume { deck, .. }
            | SetEq { deck, .. }
            | SetFilter { deck, .. }
            | SetFilterActive { deck, .. }
            | SetPlaybackRate { deck, .. }
            | SetNudge { deck, .. }
            | SetBeatGrid { deck, .. }
            | LoopIn { deck, .. }
            | LoopOut { deck, .. }
            | ExitLoop { deck }
            | Reloop { deck }
            | CuePreviewStart { deck, .. }
            | CuePreviewEnd { deck, .. } => Some(deck),
        }
    }
}

impl SessionEvent {
    // Returns None for events that are not replayed (recording_start/stop,
    // cue_move, set_cue_active, set_cue_mix, unknown types) and for events
    // missing a field they cannot be applied without.
    pub fn command(&self) -> Option<SessionCommand<'_>> {
        use SessionCommand::*;
        let deck = self.deck.as_deref();
        Some(match self.event_type.as_str() {
            "deck_snapshot" => DeckSnapshot {
                deck: deck?,
                path: self.path.as_deref()?,
                position_sec: self.position_sec,
                cue_point_sec: self.cue_point_sec,
                bpm: self.bpm,
                playback_rate: self.playback_rate,
                loop_active: self.loop_active,
                loop_end_sec: self.loop_end_sec,
                is_playing: self.is_playing == Some(true),
            },
            "load_track" => LoadTrack {
                deck: deck?,
                path: self.path.as_deref()?,
                beat_offset_sec: self.beat_offset_sec,
            },
            "eject_track" => EjectTrack { deck: deck? },
            "play" => Play {
                deck: deck?,
                sec: self.sec,
            },
            "stop" => Stop { deck: deck? },
            "stopped_at_cue" | "stop_at_cue" => StopAtCue {
                deck: deck?,
                cue_point_sec: self.cue_point_sec,
            },
            "seek" => Seek {
                deck: deck?,
                sec: self.sec?,
            },
            "set_volume" => SetVolume {
                deck: deck?,
                gain: self.gain?,
            },
            "set_eq" => SetEq {
                deck: deck?,
                band: self.band.as_deref()?,
                db: self.db?,
            },
            "set_filter" => SetFilter {
                deck: deck?,
                value: self.value?,
            },
            "set_filter_active" => SetFilterActive {
                deck: deck?,
                active: self.active?,
            },
            "set_playback_rate" => SetPlaybackRate {
                deck: deck?,
                rate: self.rate?,
            },
            "set_nudge" => SetNudge {
                deck: deck?,
                percent: self.percent?,
            },
            "set_master_gain" => SetMasterGain { gain: self.gain? },
            "set_beat_grid" => SetBeatGrid {
                deck: deck?,
                bpm: self.bpm,
                beat_offset_sec: self.beat_offset_sec,
            },
            "loop_in" => LoopIn {
                deck: deck?,
                cue_sec: self.cue_sec,
            },
            "loop_out" => LoopOut {
                deck: deck?,
                start_sec: self.start_sec,
                end_sec: self.end_sec,
            },
            "exit_loop" => ExitLoop { deck: deck? },
            "reloop" => Reloop { deck: deck? },
            "cue_preview_start" => CuePreviewStart {
                deck: deck?,
                cue_point_sec: self.cue_point_sec,
            },
            "cue_preview_end" => CuePreviewEnd {
                deck: deck?,
                cue_point_sec: self.cue_point_sec,
            },
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(event_type: &str) -> SessionEvent {
        SessionEvent {
            event_type: event_type.to_string(),
            deck: Some("A".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn every_replayable_type_converts() {
        let full = SessionEvent {
            path: Some("/a.wav".to_string()),
            sec: Some(1.0),
            gain: Some(0.5),
            band: Some("low".to_string()),
            db: Some(-6.0),
            value: Some(-0.5),
            active: Some(true),
            rate: Some(1.02),
            percent: Some(4.0),
            beat_offset_sec: Some(0.25),
            start_sec: Some(4.0),
            end_sec: Some(6.0),
            cue_point_sec: Some(1.5),
            bpm: Some(128.0),
            cue_sec: Some(2.0),
            ..ev("")
        };
        for ty in [
            "deck_snapshot",
            "load_track",
            "eject_track",
            "play",
            "stop",
            "stopped_at_cue",
            "stop_at_cue",
            "seek",
            "set_volume",
            "set_eq",
            "set_filter",
            "set_filter_active",
            "set_playback_rate",
            "set_nudge",
            "set_master_gain",
            "set_beat_grid",
            "loop_in",
            "loop_out",
            "exit_loop",
            "reloop",
            "cue_preview_start",
            "cue_preview_end",
        ] {
            let event = SessionEvent {
                event_type: ty.to_string(),
                ..full.clone()
            };
            assert!(event.command().is_some(), "{ty} did not convert");
        }
    }

    #[test]
    fn non_replayable_types_convert_to_none() {
        for ty in [
            "recording_start",
            "recording_stop",
            "cue_move",
            "set_cue_active",
            "set_cue_mix",
            "some_future_event",
        ] {
            assert!(ev(ty).command().is_none(), "{ty} should not convert");
        }
    }

    #[test]
    fn stop_at_cue_aliases_map_to_same_command() {
        let a = SessionEvent {
            cue_point_sec: Some(1.5),
            ..ev("stopped_at_cue")
        };
        let b = SessionEvent {
            cue_point_sec: Some(1.5),
            ..ev("stop_at_cue")
        };
        assert_eq!(a.command(), b.command());
    }

    #[test]
    fn missing_required_fields_convert_to_none() {
        assert!(ev("seek").command().is_none(), "seek without sec");
        assert!(ev("set_volume").command().is_none(), "set_volume w/o gain");
        assert!(ev("deck_snapshot").command().is_none(), "snapshot w/o path");
        assert!(ev("load_track").command().is_none(), "load_track w/o path");
        let no_deck = SessionEvent {
            deck: None,
            ..Default::default()
        };
        assert!(
            SessionEvent {
                event_type: "play".to_string(),
                ..no_deck
            }
            .command()
            .is_none(),
            "play without deck"
        );
    }

    #[test]
    fn optional_fields_stay_optional() {
        match ev("play").command() {
            Some(SessionCommand::Play { deck, sec }) => {
                assert_eq!(deck, "A");
                assert_eq!(sec, None);
            }
            other => panic!("unexpected: {other:?}"),
        }
        match ev("loop_in").command() {
            Some(SessionCommand::LoopIn { cue_sec: None, .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn deck_snapshot_is_playing_requires_explicit_true() {
        let base = SessionEvent {
            path: Some("/a.wav".to_string()),
            ..ev("deck_snapshot")
        };
        for (input, expected) in [(None, false), (Some(false), false), (Some(true), true)] {
            let event = SessionEvent {
                is_playing: input,
                ..base.clone()
            };
            match event.command() {
                Some(SessionCommand::DeckSnapshot { is_playing, .. }) => {
                    assert_eq!(is_playing, expected, "is_playing input {input:?}")
                }
                other => panic!("unexpected: {other:?}"),
            }
        }
    }
}
