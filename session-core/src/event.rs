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
    pub band: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db: Option<f32>,
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
    pub buffer_size_frames: Option<u32>,
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

    fn port_v1_to_v2(&mut self) -> bool {
        let deck_scoped = self.deck.is_some();
        let (slot, param, value) = match self.event_type.as_str() {
            "set_volume" if deck_scoped => ("fader".to_string(), "gain".to_string(), self.gain),
            "set_eq" if deck_scoped => match (self.band.clone(), self.db) {
                (Some(band), Some(db)) => ("eq".to_string(), band, Some(db)),
                _ => return false,
            },
            "set_filter" if deck_scoped => ("filter".to_string(), "value".to_string(), self.value),
            "set_filter_active" if deck_scoped => (
                "filter".to_string(),
                "active".to_string(),
                self.active.map(|active| if active { 1.0 } else { 0.0 }),
            ),
            // Master scope, so it carries no deck and the master slot is named "gain".
            "set_master_gain" if !deck_scoped => {
                ("gain".to_string(), "gain".to_string(), self.gain)
            }
            _ => return false,
        };
        let Some(value) = value else {
            return false;
        };
        self.event_type = "set_param".to_string();
        self.slot = Some(slot);
        self.param = Some(param);
        self.value = Some(value);
        true
    }
}

fn port_v1_events(events: &mut [SessionEvent]) -> usize {
    let mut ported = 0;
    for event in events.iter_mut() {
        if event.port_v1_to_v2() {
            ported += 1;
        }
    }
    ported
}

pub fn port_events(events: &mut [SessionEvent], from_version: u32) -> usize {
    match from_version {
        1 => port_v1_events(events),
        _ => 0,
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct SessionFile {
    pub version: u32,
    pub events: Vec<SessionEvent>,
    // Absent in sessions recorded before manifests existed.
    #[serde(default)]
    pub mixer: Option<crate::param::MixerHeader>,
}

pub const BMS_VERSION: u32 = 2;

impl SessionFile {
    /// Read a `.bms` only through here: plain deserialization drops an older session's
    /// mixer moves in silence.
    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        let mut file: Self = serde_json::from_str(json)?;
        port_events(&mut file.events, file.version);
        Ok(file)
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
    // Only this axis is string-addressed; transport stays one variant per
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
    fn the_v1_vocabulary_ports_onto_classic_slots() {
        let mut events = vec![
            SessionEvent {
                gain: Some(0.0),
                ..make_event("set_volume")
            },
            SessionEvent {
                band: Some("low".to_string()),
                db: Some(-2.5),
                ..make_event("set_eq")
            },
            SessionEvent {
                value: Some(0.35),
                ..make_event("set_filter")
            },
            SessionEvent {
                active: Some(true),
                ..make_event("set_filter_active")
            },
        ];
        assert_eq!(port_events(&mut events, 1), 4);

        assert!(events[0].is_param(Some("A"), "fader", "gain"));
        assert_eq!(events[0].value, Some(0.0));
        assert!(events[1].is_param(Some("A"), "eq", "low"));
        assert_eq!(events[1].value, Some(-2.5));
        assert!(events[2].is_param(Some("A"), "filter", "value"));
        assert_eq!(events[2].value, Some(0.35));
        assert!(events[3].is_param(Some("A"), "filter", "active"));
        assert_eq!(events[3].value, Some(1.0));

        for event in &events {
            assert!(
                event.command().is_some(),
                "{:?} still does not replay",
                event
            );
        }
    }

    #[test]
    fn every_ported_address_exists_on_the_classic_mixer() {
        let mut events = vec![
            SessionEvent {
                gain: Some(0.8),
                ..make_event("set_volume")
            },
            SessionEvent {
                band: Some("mid".to_string()),
                db: Some(3.0),
                ..make_event("set_eq")
            },
            SessionEvent {
                band: Some("high".to_string()),
                db: Some(3.0),
                ..make_event("set_eq")
            },
            SessionEvent {
                value: Some(-0.5),
                ..make_event("set_filter")
            },
            SessionEvent {
                active: Some(false),
                ..make_event("set_filter_active")
            },
        ];
        port_events(&mut events, 1);
        let manifest = crate::param::resolve_manifest(None).expect("a headerless session");
        for event in &events {
            let slot = event.slot.as_deref().expect("ported to a slot");
            let param = event.param.as_deref().expect("ported to a param");
            assert!(
                manifest
                    .descriptor(crate::ParamScope::Deck, slot, param)
                    .is_some(),
                "{slot}/{param} is not on {}",
                manifest.id
            );
        }
    }

    #[test]
    fn the_current_vocabulary_is_left_alone() {
        let mut events = vec![
            SessionEvent::param(0.0, Some("A"), "eq", "low", -6.0),
            make_event("play"),
            make_event("set_nudge"),
        ];
        let before = serde_json::to_string(&events).expect("serialize");
        assert_eq!(port_events(&mut events, 1), 0);
        assert_eq!(serde_json::to_string(&events).expect("serialize"), before);
    }

    #[test]
    fn a_v1_event_missing_its_value_is_not_ported() {
        let mut events = vec![
            make_event("set_volume"),
            SessionEvent {
                db: Some(-3.0),
                ..make_event("set_eq")
            },
            SessionEvent {
                gain: Some(0.5),
                deck: None,
                ..make_event("set_volume")
            },
        ];
        assert_eq!(port_events(&mut events, 1), 0);
        assert_eq!(events[0].event_type, "set_volume");
        assert_eq!(events[1].event_type, "set_eq");
        assert_eq!(events[2].event_type, "set_volume");
    }

    #[test]
    fn the_v1_master_gain_ports_onto_the_master_slot() {
        let mut events = vec![SessionEvent {
            gain: Some(0.6),
            deck: None,
            ..make_event("set_master_gain")
        }];
        assert_eq!(port_events(&mut events, 1), 1);
        assert!(events[0].is_param(None, "gain", "gain"));
        assert_eq!(events[0].value, Some(0.6));
        assert!(events[0].command().is_some(), "it has to replay");
    }

    #[test]
    fn a_deck_scoped_v1_event_still_needs_its_deck() {
        let mut events = vec![SessionEvent {
            gain: Some(0.5),
            deck: None,
            ..make_event("set_volume")
        }];
        assert_eq!(port_events(&mut events, 1), 0);
        assert_eq!(events[0].event_type, "set_volume");
    }

    #[test]
    fn an_explicit_null_deck_reads_as_no_deck() {
        let mut events: Vec<SessionEvent> = serde_json::from_str(
            r#"[
                {"elapsed_ms": 1, "type": "set_volume", "deck": null, "gain": 0.5},
                {"elapsed_ms": 2, "type": "set_master_gain", "deck": null, "gain": 0.6}
            ]"#,
        )
        .expect("a session with null decks");
        assert_eq!(port_events(&mut events, 1), 1);
        assert_eq!(events[0].event_type, "set_volume");
        assert!(events[1].is_param(None, "gain", "gain"));
    }

    #[test]
    fn a_current_version_is_never_ported() {
        let mut events = vec![SessionEvent {
            gain: Some(0.5),
            ..make_event("set_volume")
        }];
        assert_eq!(port_events(&mut events, BMS_VERSION), 0);
        assert_eq!(events[0].event_type, "set_volume");
    }

    #[test]
    fn parsing_a_session_ports_it() {
        let json = r#"{"version":1,"events":[
            {"elapsed_ms":1.0,"type":"set_volume","deck":"A","gain":0.25},
            {"elapsed_ms":2.0,"type":"set_eq","deck":"B","band":"high","db":2.0}
        ]}"#;
        let session = SessionFile::parse(json).expect("parse");
        assert!(session.events[0].is_param(Some("A"), "fader", "gain"));
        assert!(session.events[1].is_param(Some("B"), "eq", "high"));
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
