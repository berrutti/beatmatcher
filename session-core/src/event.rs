use crate::param::ParamScope;

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
    pub slot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assign: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticks: Option<f64>,
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
    pub buffer_size_frames: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limiter_enabled: Option<bool>,
    /// The device rate `frame` counts in, so a render at another rate can scale it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    /// Output frames since capture began: which buffer the command landed in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<u64>,
}

impl SessionEvent {
    pub fn synthesized_at(&self, elapsed_ms: f64) -> SessionEvent {
        SessionEvent {
            elapsed_ms,
            frame: None,
            ..self.clone()
        }
    }

    pub fn at(elapsed_ms: f64, event_type: &str, deck: &str) -> SessionEvent {
        SessionEvent {
            elapsed_ms,
            event_type: event_type.to_string(),
            deck: Some(deck.to_string()),
            ..Default::default()
        }
    }

    // `deck` is None for master scope, which is how the scope is inferred back.
    pub fn param(
        elapsed_ms: f64,
        deck: Option<&str>,
        slot: &str,
        param: &str,
        value: f64,
    ) -> SessionEvent {
        SessionEvent {
            elapsed_ms,
            event_type: "set_param".to_string(),
            deck: deck.map(str::to_string),
            slot: Some(slot.to_string()),
            param: Some(param.to_string()),
            value: Some(value as f32),
            ..Default::default()
        }
    }

    pub fn is_param(&self, deck: Option<&str>, slot: &str, param: &str) -> bool {
        self.event_type == "set_param"
            && self.deck.as_deref() == deck
            && self.slot.as_deref() == Some(slot)
            && self.param.as_deref() == Some(param)
    }

}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct SessionFile {
    pub version: u32,
    pub events: Vec<SessionEvent>,
    pub mixer: crate::param::MixerHeader,
}

pub const BMS_VERSION: u32 = 2;

impl SessionFile {
    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
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
    // Only this axis is string-addressed. Transport stays one variant per
    // command so the three interpreters fail to compile when one is added.
    SetParam {
        scope: ParamScope,
        deck: Option<&'a str>,
        slot: &'a str,
        param: &'a str,
        value: f64,
    },
    // Categorical, so it gets its own variant rather than riding SetParam as a number.
    // Same reasoning as `set_cue_active`: per-strip state the manifest does not describe.
    SetXfaderAssign {
        deck: &'a str,
        assign: crate::XfaderAssign,
    },
    // Categorical for the same reason, and master scope: one switch sets the
    // taper of every channel fader.
    SetFaderCurve {
        curve: crate::FaderCurve,
    },
    // The wheel's own input rather than its effect, which is computed per audio block and
    // so is never known on the thread that logs.
    Jog {
        deck: &'a str,
        ticks: f64,
    },
    // Categorical, and it decides what one logged tick is worth, so a session that omits
    // it cannot be replayed at the speed it was played on.
    SetJogRotationSpeed {
        speed: crate::JogRotationSpeed,
    },
    SetPlaybackRate {
        deck: &'a str,
        rate: f64,
    },
    SetNudge {
        deck: &'a str,
        percent: f64,
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
    /// The event type and payload that record this command. Derived here so a
    /// recorder cannot write something other than what was applied.
    pub fn to_event(&self) -> (&'static str, serde_json::Value) {
        use SessionCommand::*;
        let deck = self.deck_id();
        let mut payload = serde_json::Map::new();
        if let Some(deck) = deck {
            payload.insert("deck".into(), serde_json::json!(deck));
        }
        let mut put = |key: &str, value: serde_json::Value| {
            payload.insert(key.into(), value);
        };
        let event_type = match *self {
            DeckSnapshot {
                path,
                position_sec,
                cue_point_sec,
                bpm,
                playback_rate,
                loop_active,
                loop_end_sec,
                is_playing,
                ..
            } => {
                put("path", serde_json::json!(path));
                put("position_sec", serde_json::json!(position_sec));
                put("cue_point_sec", serde_json::json!(cue_point_sec));
                put("bpm", serde_json::json!(bpm));
                put("playback_rate", serde_json::json!(playback_rate));
                put("loop_active", serde_json::json!(loop_active));
                put("loop_end_sec", serde_json::json!(loop_end_sec));
                put("is_playing", serde_json::json!(is_playing));
                "deck_snapshot"
            }
            LoadTrack {
                path,
                beat_offset_sec,
                ..
            } => {
                put("path", serde_json::json!(path));
                put("beat_offset_sec", serde_json::json!(beat_offset_sec));
                "load_track"
            }
            EjectTrack { .. } => "eject_track",
            Play { sec, .. } => {
                if let Some(sec) = sec {
                    put("sec", serde_json::json!(sec));
                }
                "play"
            }
            Stop { .. } => "stop",
            StopAtCue { cue_point_sec, .. } => {
                put("cue_point_sec", serde_json::json!(cue_point_sec));
                "stopped_at_cue"
            }
            Seek { sec, .. } => {
                put("sec", serde_json::json!(sec));
                "seek"
            }
            SetParam {
                slot,
                param,
                value,
                scope,
                ..
            } => {
                put("slot", serde_json::json!(slot));
                put("param", serde_json::json!(param));
                put("value", serde_json::json!(value));
                let _ = scope;
                "set_param"
            }
            SetXfaderAssign { assign, .. } => {
                put("assign", serde_json::json!(assign.as_str()));
                "set_xfader_assign"
            }
            SetFaderCurve { curve } => {
                put("curve", serde_json::json!(curve.as_str()));
                "set_fader_curve"
            }
            Jog { ticks, .. } => {
                put("ticks", serde_json::json!(ticks));
                "jog"
            }
            SetJogRotationSpeed { speed } => {
                put("speed", serde_json::json!(speed.as_str()));
                "set_jog_rotation_speed"
            }
            SetPlaybackRate { rate, .. } => {
                put("rate", serde_json::json!(rate));
                "set_playback_rate"
            }
            SetNudge { percent, .. } => {
                put("percent", serde_json::json!(percent));
                "set_nudge"
            }
            SetBeatGrid {
                bpm,
                beat_offset_sec,
                ..
            } => {
                put("bpm", serde_json::json!(bpm));
                put("beat_offset_sec", serde_json::json!(beat_offset_sec));
                "set_beat_grid"
            }
            LoopIn { cue_sec, .. } => {
                put("cue_sec", serde_json::json!(cue_sec));
                "loop_in"
            }
            LoopOut {
                start_sec, end_sec, ..
            } => {
                put("start_sec", serde_json::json!(start_sec));
                put("end_sec", serde_json::json!(end_sec));
                "loop_out"
            }
            ExitLoop { .. } => "exit_loop",
            Reloop { .. } => "reloop",
            CuePreviewStart { cue_point_sec, .. } => {
                put("cue_point_sec", serde_json::json!(cue_point_sec));
                "cue_preview_start"
            }
            CuePreviewEnd { cue_point_sec, .. } => {
                put("cue_point_sec", serde_json::json!(cue_point_sec));
                "cue_preview_end"
            }
        };
        (event_type, serde_json::Value::Object(payload))
    }

    // The deck a command targets, or None for commands that act on the master strip.
    pub fn deck_id(&self) -> Option<&'a str> {
        use SessionCommand::*;
        match *self {
            SetParam { deck, .. } => deck,
            SetFaderCurve { .. } | SetJogRotationSpeed { .. } => None,
            DeckSnapshot { deck, .. }
            | LoadTrack { deck, .. }
            | EjectTrack { deck }
            | Play { deck, .. }
            | Stop { deck }
            | StopAtCue { deck, .. }
            | Seek { deck, .. }
            | SetXfaderAssign { deck, .. }
            | Jog { deck, .. }
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
            "stopped_at_cue" => StopAtCue {
                deck: deck?,
                cue_point_sec: self.cue_point_sec,
            },
            "seek" => Seek {
                deck: deck?,
                sec: self.sec?,
            },
            "set_param" => SetParam {
                scope: if self.deck.is_some() {
                    ParamScope::Deck
                } else {
                    ParamScope::Master
                },
                deck,
                slot: self.slot.as_deref()?,
                param: self.param.as_deref()?,
                value: self.value? as f64,
            },
            "set_xfader_assign" => SetXfaderAssign {
                deck: deck?,
                assign: crate::XfaderAssign::from_str_or_thru(self.assign.as_deref()?),
            },
            "set_fader_curve" => SetFaderCurve {
                curve: crate::FaderCurve::from_str_or_linear(self.curve.as_deref()?),
            },
            "jog" => Jog {
                deck: deck?,
                ticks: self.ticks?,
            },
            "set_jog_rotation_speed" => SetJogRotationSpeed {
                speed: crate::JogRotationSpeed::from_str_or_33(self.speed.as_deref()?),
            },
            "set_playback_rate" => SetPlaybackRate {
                deck: deck?,
                rate: self.rate?,
            },
            "set_nudge" => SetNudge {
                deck: deck?,
                percent: self.percent?,
            },
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

    fn make_event(event_type: &str) -> SessionEvent {
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
            ..make_event("")
        };
        for event_type in [
            "deck_snapshot",
            "load_track",
            "eject_track",
            "play",
            "stop",
            "stopped_at_cue",
            "seek",
            "set_playback_rate",
            "set_nudge",
            "set_beat_grid",
            "loop_in",
            "loop_out",
            "exit_loop",
            "reloop",
            "cue_preview_start",
            "cue_preview_end",
        ] {
            let event = SessionEvent {
                event_type: event_type.to_string(),
                ..full.clone()
            };
            assert!(event.command().is_some(), "{event_type} did not convert");
        }
    }

    #[test]
    fn non_replayable_types_convert_to_none() {
        for event_type in [
            "recording_start",
            "recording_stop",
            "cue_move",
            "set_cue_active",
            "set_cue_mix",
            "some_future_event",
        ] {
            assert!(
                make_event(event_type).command().is_none(),
                "{event_type} should not convert"
            );
        }
    }

    #[test]
    fn missing_required_fields_convert_to_none() {
        assert!(make_event("seek").command().is_none(), "seek without sec");
        assert!(
            make_event("set_param").command().is_none(),
            "set_param w/o slot"
        );
        assert!(
            make_event("deck_snapshot").command().is_none(),
            "snapshot w/o path"
        );
        assert!(
            make_event("load_track").command().is_none(),
            "load_track w/o path"
        );
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
        match make_event("play").command() {
            Some(SessionCommand::Play { deck, sec }) => {
                assert_eq!(deck, "A");
                assert_eq!(sec, None);
            }
            other => panic!("unexpected: {other:?}"),
        }
        match make_event("loop_in").command() {
            Some(SessionCommand::LoopIn { cue_sec: None, .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn deck_snapshot_is_playing_requires_explicit_true() {
        let base = SessionEvent {
            path: Some("/a.wav".to_string()),
            ..make_event("deck_snapshot")
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

#[cfg(test)]
mod command_events {
    use super::*;

    fn round_trip(command: SessionCommand<'_>) -> Option<SessionCommand<'static>> {
        let (event_type, payload) = command.to_event();
        let mut object = match payload {
            serde_json::Value::Object(fields) => fields,
            _ => panic!("a command's payload is an object"),
        };
        object.insert("type".into(), serde_json::json!(event_type));
        object.insert("elapsed_ms".into(), serde_json::json!(0.0));
        let event: SessionEvent =
            serde_json::from_value(serde_json::Value::Object(object)).expect("parses back");
        // Leaked so the borrowed command can outlive the event it came from,
        // which only a test needs to do.
        Box::leak(Box::new(event)).command()
    }

    #[test]
    fn a_logged_command_reads_back_as_the_command_that_was_applied() {
        let deck = "A";
        for command in [
            SessionCommand::Play { deck, sec: None },
            SessionCommand::Play {
                deck,
                sec: Some(12.5),
            },
            SessionCommand::Stop { deck },
            SessionCommand::StopAtCue {
                deck,
                cue_point_sec: Some(3.25),
            },
            SessionCommand::Seek { deck, sec: 7.5 },
            SessionCommand::SetPlaybackRate { deck, rate: 1.03 },
            SessionCommand::SetNudge {
                deck,
                percent: -4.0,
            },
            SessionCommand::EjectTrack { deck },
            SessionCommand::SetBeatGrid {
                deck,
                bpm: Some(128.0),
                beat_offset_sec: Some(0.25),
            },
            SessionCommand::LoopIn {
                deck,
                cue_sec: Some(2.0),
            },
            SessionCommand::LoopOut {
                deck,
                start_sec: Some(2.0),
                end_sec: Some(6.0),
            },
            SessionCommand::ExitLoop { deck },
            SessionCommand::Reloop { deck },
            SessionCommand::CuePreviewStart {
                deck,
                cue_point_sec: Some(1.5),
            },
            SessionCommand::CuePreviewEnd {
                deck,
                cue_point_sec: Some(1.5),
            },
            SessionCommand::Jog { deck, ticks: 12.0 },
            SessionCommand::DeckSnapshot {
                deck,
                path: "/music/a.mp3",
                position_sec: Some(12.5),
                cue_point_sec: Some(1.25),
                bpm: Some(128.0),
                playback_rate: Some(0.98),
                loop_active: Some(true),
                loop_end_sec: Some(20.0),
                is_playing: true,
            },
            SessionCommand::LoadTrack {
                deck,
                path: "/music/b.mp3",
                beat_offset_sec: Some(0.5),
            },
        ] {
            assert_eq!(round_trip(command), Some(command), "{command:?}");
        }
    }
}
