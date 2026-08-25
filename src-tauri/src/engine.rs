use crate::audio::{self, AppAudio, ChannelStrip, Deck};
use crate::deck_sync::DeckSyncPayload;
use crate::engine_push::{EnginePush, ParamOrigin};
use crate::lock::LockIgnoringPoison;
use crate::rate_from_fader;
use crate::recorder::Recorder;
use crate::MIN_PLAYBACK_RATE;
use std::sync::Arc;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoopOutResult {
    pub(crate) start_sec: f64,
    pub(crate) end_sec: f64,
    pub(crate) beats: i64,
    // Some when a late quantized press seeked, so the frontend resyncs its position cache.
    pub(crate) seek_to_sec: Option<f64>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NudgeResult {
    pub(crate) position_sec: f64,
    pub(crate) effective_rate: f64,
}

pub(crate) fn quantize_to_beat(pos_frames: f64, bpm: f64, beat_offset_frames: f64, sr: f64) -> f64 {
    let beat_dur = (60.0 / bpm) * sr;
    let index = ((pos_frames - beat_offset_frames) / beat_dur).round();
    (beat_offset_frames + index * beat_dur).max(0.0)
}

pub(crate) fn loop_in_core(deck_state: &mut Deck, strip: &mut ChannelStrip) -> Result<f64, String> {
    let sr = deck_state.device_sample_rate as f64;
    let bpm = deck_state.bpm.ok_or("no beat grid set")?;
    let in_frames = if deck_state.quantize {
        quantize_to_beat(deck_state.main_pos, bpm, deck_state.beat_offset_frames, sr)
    } else {
        deck_state.main_pos
    };
    let cue_sec = in_frames / sr;
    let sample_rate = deck_state.device_sample_rate;
    audio::apply_deck_command(
        &session_core::SessionCommand::LoopIn {
            deck: "",
            cue_sec: Some(cue_sec),
        },
        deck_state,
        strip,
        sample_rate,
        0.0,
        &mut |path: &str| Err(format!("a live command cannot load {path}")),
    )?;
    Ok(cue_sec)
}

pub(crate) fn loop_out_core(
    deck_state: &mut Deck,
    strip: &mut ChannelStrip,
) -> Result<Option<LoopOutResult>, String> {
    let sr = deck_state.device_sample_rate as f64;
    let deck_state_sample_rate = deck_state.device_sample_rate;
    let bpm = deck_state.bpm.ok_or("no beat grid set")?;
    let out_frames = if deck_state.quantize {
        quantize_to_beat(deck_state.main_pos, bpm, deck_state.beat_offset_frames, sr)
    } else {
        deck_state.main_pos
    };
    let in_frames = deck_state.cue_point;
    if out_frames <= in_frames {
        return Ok(None);
    }
    let start_sec = in_frames / sr;
    let end_sec = out_frames / sr;
    let was_past_end = deck_state.quantize && deck_state.main_pos > out_frames;
    audio::apply_deck_command(
        &session_core::SessionCommand::LoopOut {
            deck: "",
            start_sec: Some(start_sec),
            end_sec: Some(end_sec),
        },
        deck_state,
        strip,
        deck_state_sample_rate,
        0.0,
        &mut |path: &str| Err(format!("a live command cannot load {path}")),
    )?;
    let seek_to_sec = was_past_end.then(|| deck_state.main_pos / sr);
    let beats = crate::deck_sync::beats_between(start_sec, end_sec, bpm);
    Ok(Some(LoopOutResult {
        start_sec,
        end_sec,
        beats,
        seek_to_sec,
    }))
}

/// The live mixer and its decks, plus the two things every write to them also does:
/// record it and mirror it back to the surface that did not move.
pub struct Engine {
    pub(crate) audio: Arc<AppAudio>,
    pub(crate) recorder: Recorder,
    pub(crate) engine_push: Arc<EnginePush>,
}

impl Engine {
    #[cfg(test)]
    pub(crate) fn for_testing(device_sample_rate: u32) -> Self {
        Self::new(Arc::new(AppAudio::unrouted(
            device_sample_rate,
            String::new(),
        )))
    }

    pub(crate) fn new(audio: Arc<AppAudio>) -> Self {
        Self {
            audio,
            recorder: Recorder::new(),
            engine_push: Arc::new(EnginePush::new()),
        }
    }

    /// The one path a deck param changes through, whoever moved it. Logging here puts a MIDI
    /// move in the `.bms` on the same terms as a mouse move.
    pub(crate) fn set_deck_param(
        &self,
        origin: ParamOrigin,
        deck: &str,
        slot: &str,
        param: &str,
        value: f32,
    ) -> Result<(), String> {
        let strip_arc = self
            .audio
            .strip(deck)
            .ok_or_else(|| format!("unknown deck: {}", deck))?;
        // Read under the strip lock, which the audio callback also takes, so the count
        // cannot advance between the mixer changing and the frame being noted.
        let frame = {
            let mut strip = strip_arc.locked();
            // A 14-bit control resolves a move on each half, so the same value arrives twice per
            // physical move. Logging both would write an event nothing can hear.
            if strip.param(slot, param) == Some(value) {
                return Ok(());
            }
            // An address no unit answers would reach the `.bms` and be dropped by every reader.
            if !strip.set_param(slot, param, value) {
                return Err(format!("unknown param: {slot}/{param}"));
            }
            strip.render_frame()
        };
        self.recorder
            .log_param_at(frame, Some(deck), slot, param, value);
        self.engine_push.mark(origin, deck, slot, param);
        Ok(())
    }

    /// Runs a command through `apply_deck_command`, the one implementation every interpreter
    /// uses, and logs the same command. A live gesture and its recording cannot describe
    /// different mutations because only one of them exists.
    fn apply_and_log(
        &self,
        deck: &str,
        cmd: &session_core::SessionCommand<'_>,
    ) -> Result<DeckSyncPayload, String> {
        let deck_arc = self.deck(deck)?;
        let strip_arc = self
            .audio
            .strip(deck)
            .ok_or_else(|| format!("unknown deck: {deck}"))?;
        let (sync, frame) = {
            let mut deck_state = deck_arc.locked();
            let mut strip = strip_arc.locked();
            let sample_rate = deck_state.device_sample_rate;
            // A live gesture is applied to the deck the caller is looking at, so there is
            // nothing to catch up with and no track to load.
            let mut refuse_load = |path: &str| -> Result<crate::audio::LoadedSamples, String> {
                Err(format!("a live command cannot load {path}"))
            };
            audio::apply_deck_command(
                cmd,
                &mut deck_state,
                &mut strip,
                sample_rate,
                0.0,
                &mut refuse_load,
            )?;
            (
                DeckSyncPayload::from_deck(&deck_state, false),
                deck_state.render_frame(),
            )
        };
        let (event_type, payload) = cmd.to_event();
        self.recorder.log_at(frame, event_type, payload);
        Ok(sync)
    }

    pub(crate) fn seek(&self, deck: &str, sec: f64) -> Result<DeckSyncPayload, String> {
        self.apply_and_log(deck, &session_core::SessionCommand::Seek { deck, sec })
    }

    pub(crate) fn stop(&self, deck: &str) -> Result<(), String> {
        self.apply_and_log(deck, &session_core::SessionCommand::Stop { deck })?;
        Ok(())
    }

    pub(crate) fn set_nudge(&self, deck: &str, percent: f64) -> Result<NudgeResult, String> {
        self.apply_and_log(
            deck,
            &session_core::SessionCommand::SetNudge { deck, percent },
        )?;
        let deck_arc = self.deck(deck)?;
        let deck_state = deck_arc.locked();
        Ok(NudgeResult {
            position_sec: deck_state.position_sec(),
            effective_rate: deck_state.playback_rate * deck_state.jog_hold_factor,
        })
    }

    pub(crate) fn eject_track(&self, deck: &str) -> Result<(), String> {
        self.apply_and_log(deck, &session_core::SessionCommand::EjectTrack { deck })?;
        Ok(())
    }

    pub(crate) fn set_beat_grid(
        &self,
        deck: &str,
        bpm: f64,
        beat_offset_sec: f64,
    ) -> Result<(), String> {
        self.apply_and_log(
            deck,
            &session_core::SessionCommand::SetBeatGrid {
                deck,
                bpm: Some(bpm),
                beat_offset_sec: Some(beat_offset_sec),
            },
        )?;
        Ok(())
    }

    pub(crate) fn deck(&self, deck: &str) -> Result<Arc<std::sync::Mutex<audio::Deck>>, String> {
        self.audio
            .deck(deck)
            .ok_or_else(|| format!("unknown deck: {}", deck))
    }

    /// The transport counterpart of `set_deck_param`, so a controller press is logged and
    /// mirrored to the UI on the same terms as a click.
    pub(crate) fn toggle_play(
        &self,
        origin: ParamOrigin,
        deck: &str,
    ) -> Result<DeckSyncPayload, String> {
        // Resolving which of the two commands this press means is live-only. Applying it is not.
        let starts_playing = {
            let deck_arc = self.deck(deck)?;
            let deck_state = deck_arc.locked();
            deck_state.total_frames > 0 && (deck_state.is_cueing || !deck_state.is_playing)
        };
        let cmd = if starts_playing {
            session_core::SessionCommand::Play { deck, sec: None }
        } else {
            session_core::SessionCommand::Stop { deck }
        };
        let payload = self.apply_and_log(deck, &cmd)?;
        self.engine_push
            .mark_transport(origin, deck, payload.loop_region_cleared);
        Ok(payload)
    }

    pub(crate) fn press_cue(
        &self,
        origin: ParamOrigin,
        deck: &str,
    ) -> Result<DeckSyncPayload, String> {
        let deck_arc = self.deck(deck)?;
        // Quantising the playhead is part of resolving which press this is, so it happens
        // before the outcome is read.
        let (outcome, had_loop) = {
            let mut deck_state = deck_arc.locked();
            if deck_state.quantize {
                if let Some(bpm) = deck_state.bpm {
                    let sr = deck_state.device_sample_rate as f64;
                    deck_state.main_pos = quantize_to_beat(
                        deck_state.main_pos,
                        bpm,
                        deck_state.beat_offset_frames,
                        sr,
                    );
                }
            }
            (deck_state.resolve_cue_press(), deck_state.loop_end > 0.0)
        };
        let cmd = match outcome {
            audio::CuePressOutcome::NoTrack => {
                let deck_state = deck_arc.locked();
                return Ok(DeckSyncPayload::from_deck(&deck_state, false));
            }
            audio::CuePressOutcome::PreviewStarted => {
                let sec = {
                    let deck_state = deck_arc.locked();
                    deck_state.cue_point / deck_state.device_sample_rate as f64
                };
                session_core::SessionCommand::CuePreviewStart {
                    deck,
                    cue_point_sec: Some(sec),
                }
            }
            // No `SessionCommand` moves a cue point: the renderer takes it from the
            // events that carry one, so this branch stays a direct write.
            audio::CuePressOutcome::CueMoved { new_cue_point_sec } => {
                let (payload, frame) = {
                    let mut deck_state = deck_arc.locked();
                    deck_state.press_cue();
                    if had_loop {
                        deck_state.loop_active = false;
                        deck_state.loop_end = 0.0;
                    }
                    (
                        DeckSyncPayload::from_deck(&deck_state, had_loop),
                        deck_state.render_frame(),
                    )
                };
                self.recorder.log_at(
                    frame,
                    "cue_move",
                    serde_json::json!({ "deck": deck, "cue_point_sec": new_cue_point_sec }),
                );
                self.engine_push
                    .mark_transport(origin, deck, payload.loop_region_cleared);
                return Ok(payload);
            }
            audio::CuePressOutcome::StoppedAtCue { cue_point_sec } => {
                session_core::SessionCommand::StopAtCue {
                    deck,
                    cue_point_sec: Some(cue_point_sec),
                }
            }
        };
        let payload = self.apply_and_log(deck, &cmd)?;
        self.engine_push
            .mark_transport(origin, deck, payload.loop_region_cleared);
        Ok(payload)
    }

    pub(crate) fn release_cue(
        &self,
        origin: ParamOrigin,
        deck: &str,
    ) -> Result<DeckSyncPayload, String> {
        let deck_arc = self.deck(deck)?;
        let (was_cueing, cue_point_sec) = {
            let deck_state = deck_arc.locked();
            (
                deck_state.is_cueing,
                deck_state.cue_point / deck_state.device_sample_rate as f64,
            )
        };
        if !was_cueing {
            let deck_state = deck_arc.locked();
            return Ok(DeckSyncPayload::from_deck(&deck_state, false));
        }
        let payload = self.apply_and_log(
            deck,
            &session_core::SessionCommand::CuePreviewEnd {
                deck,
                cue_point_sec: Some(cue_point_sec),
            },
        )?;
        self.engine_push
            .mark_transport(origin, deck, payload.loop_region_cleared);
        Ok(payload)
    }

    pub(crate) fn set_playback_rate(
        &self,
        origin: ParamOrigin,
        deck: &str,
        rate: f64,
    ) -> Result<(), String> {
        self.apply_and_log(
            deck,
            &session_core::SessionCommand::SetPlaybackRate { deck, rate },
        )?;
        self.engine_push.mark_rate(origin, deck);
        Ok(())
    }

    pub(crate) fn set_playback_rate_from_fader(
        &self,
        origin: ParamOrigin,
        deck: &str,
        position: f64,
    ) -> Result<(), String> {
        // Clamped here too, so a rate the deck would refuse still compares equal.
        let rate =
            rate_from_fader(position, self.audio.pitch_range_percent()).max(MIN_PLAYBACK_RATE);
        // Several fader positions land on one step, so quantizing alone repeats values.
        if self.deck(deck)?.locked().playback_rate == rate {
            return Ok(());
        }
        self.set_playback_rate(origin, deck, rate)
    }

    /// Wheel ticks accumulate on the deck and the audio thread consumes them, so this waits
    /// for no block. The transport mark makes the next flush read whatever the wheel moved.
    pub(crate) fn jog(&self, origin: ParamOrigin, deck: &str, ticks: i32) -> Result<(), String> {
        // Shift is scaled here rather than at consume time so the logged ticks are exactly
        // the ones the engine acts on, and a replay needs no shift state of its own.
        let (scaled, frame) = {
            let deck_arc = self.deck(deck)?;
            let mut deck_state = deck_arc.locked();
            let scaled = crate::audio::logged_jog_ticks(
                f64::from(ticks),
                deck_state.jog_shift,
                deck_state.is_playing,
            );
            deck_state.queue_jog(scaled);
            (scaled, deck_state.render_frame())
        };
        self.recorder.log_at(
            frame,
            "jog",
            serde_json::json!({ "deck": deck, "ticks": scaled }),
        );
        self.engine_push.mark_transport(origin, deck, false);
        Ok(())
    }

    /// Held on the surface rather than latched, so it is set on both edges and
    /// moves nothing on its own.
    pub(crate) fn set_jog_shift(&self, deck: &str, held: bool) -> Result<(), String> {
        self.deck(deck)?.locked().set_jog_shift(held);
        Ok(())
    }

    pub(crate) fn loop_in(
        &self,
        origin: ParamOrigin,
        deck: &str,
    ) -> Result<DeckSyncPayload, String> {
        let deck_arc = self.deck(deck)?;
        let strip_arc = self
            .audio
            .strip(deck)
            .ok_or_else(|| format!("unknown deck: {deck}"))?;
        let (payload, cue_sec, quantize, frame) = {
            let mut deck_state = deck_arc.locked();
            let mut strip = strip_arc.locked();
            let sec = loop_in_core(&mut deck_state, &mut strip)?;
            let payload = DeckSyncPayload::from_deck(&deck_state, true);
            (payload, sec, deck_state.quantize, deck_state.render_frame())
        };
        self.recorder.log_at(
            frame,
            "loop_in",
            serde_json::json!({ "deck": deck, "cue_sec": cue_sec, "quantized": quantize }),
        );
        self.engine_push
            .mark_transport(origin, deck, payload.loop_region_cleared);
        Ok(payload)
    }

    pub(crate) fn loop_out(
        &self,
        origin: ParamOrigin,
        deck: &str,
    ) -> Result<Option<LoopOutResult>, String> {
        let deck_arc = self.deck(deck)?;
        let strip_arc = self
            .audio
            .strip(deck)
            .ok_or_else(|| format!("unknown deck: {deck}"))?;
        let (result, quantize, frame) = {
            let mut deck_state = deck_arc.locked();
            let mut strip = strip_arc.locked();
            let quantize = deck_state.quantize;
            (
                loop_out_core(&mut deck_state, &mut strip)?,
                quantize,
                deck_state.render_frame(),
            )
        };
        if let Some(region) = &result {
            self.recorder.log_at(
                frame,
                "loop_out",
                serde_json::json!({
                    "deck": deck,
                    "start_sec": region.start_sec,
                    "end_sec": region.end_sec,
                    "beats": region.beats,
                    "quantized": quantize,
                }),
            );
            self.engine_push.mark_transport(origin, deck, false);
        }
        Ok(result)
    }

    pub(crate) fn set_loop_active(
        &self,
        origin: ParamOrigin,
        deck: &str,
        active: bool,
    ) -> Result<DeckSyncPayload, String> {
        let payload = if active {
            let deck_arc = self.deck(deck)?;
            let mut deck_state = deck_arc.locked();
            deck_state.loop_active = true;
            DeckSyncPayload::from_deck(&deck_state, false)
        } else {
            self.apply_and_log(deck, &session_core::SessionCommand::ExitLoop { deck })?
        };
        self.engine_push.mark_transport(origin, deck, false);
        Ok(payload)
    }

    pub(crate) fn reloop(
        &self,
        origin: ParamOrigin,
        deck: &str,
    ) -> Result<DeckSyncPayload, String> {
        let payload = self.apply_and_log(deck, &session_core::SessionCommand::Reloop { deck })?;
        self.engine_push.mark_transport(origin, deck, false);
        Ok(payload)
    }

    /// The controller's third loop button, which the keyboard reaches with shift over loop
    /// out. Resolved from engine state rather than mirrored from the UI.
    pub(crate) fn exit_or_reloop(
        &self,
        origin: ParamOrigin,
        deck: &str,
    ) -> Result<DeckSyncPayload, String> {
        let deck_arc = self.deck(deck)?;
        let (active, has_region) = {
            let deck_state = deck_arc.locked();
            (
                deck_state.loop_active,
                deck_state.loop_end > deck_state.cue_point,
            )
        };
        if active {
            return self.set_loop_active(origin, deck, false);
        }
        if has_region {
            return self.reloop(origin, deck);
        }
        let deck_state = deck_arc.locked();
        Ok(DeckSyncPayload::from_deck(&deck_state, false))
    }

    /// Cue is headphone routing rather than a mixer move, so it skips `set_deck_param`.
    /// It is still logged, under its own event type.
    fn apply_cue_active(&self, origin: ParamOrigin, deck: &str, active: bool) {
        let frame = self
            .audio
            .strip(deck)
            .map(|strip| strip.locked().render_frame())
            .unwrap_or_else(|| self.master_frame());
        self.recorder.log_at(
            frame,
            "set_cue_active",
            serde_json::json!({ "deck": deck, "active": active }),
        );
        self.engine_push.mark_cue(origin, deck);
    }

    /// Position is one master value, but the gain it implies is per strip, so
    /// every strip is re-resolved against its own assign on each move.
    pub(crate) fn set_xfader_position(&self, origin: ParamOrigin, position: f32) {
        // A 14-bit control resolves a move on each half, so one sweep would otherwise
        // re-resolve every strip and log an event thousands of times over.
        if !self.audio.monitor.set_xfader_position(position) {
            return;
        }
        let frame = self
            .resolve_xfader_gains()
            .unwrap_or_else(|| self.master_frame());
        let landed = self.audio.monitor.xfader_position();
        self.recorder
            .log_param_at(frame, None, "xfader", "position", landed);
        self.engine_push.mark_xfader(origin);
    }

    pub(crate) fn set_xfader_assign(
        &self,
        origin: ParamOrigin,
        deck: &str,
        assign: session_core::XfaderAssign,
    ) -> Result<(), String> {
        let frame = {
            let strip_arc = self
                .audio
                .strip(deck)
                .ok_or_else(|| format!("unknown deck: {}", deck))?;
            let mut strip = strip_arc.locked();
            strip.set_xfader_assign(assign);
            strip.render_frame()
        };
        self.recorder.log_at(
            frame,
            "set_xfader_assign",
            serde_json::json!({ "deck": deck, "assign": assign.as_str() }),
        );
        self.engine_push.mark_xfader_assign(origin, deck);
        Ok(())
    }

    /// One switch, every channel, as on a mixer: the strips each hold their own
    /// copy because the taper is applied before the fader's own smoothing.
    pub(crate) fn set_master_gain(&self, gain: f32) {
        self.audio.monitor.set_master_gain(gain);
        self.recorder
            .log_param_at(self.master_frame(), None, "gain", "gain", gain);
    }

    pub(crate) fn set_fader_curve(&self, curve: session_core::FaderCurve) {
        self.audio.monitor.set_fader_curve(curve);
        let frame = self
            .for_each_strip(|strip| strip.set_fader_curve(curve))
            .unwrap_or_else(|| {
                crate::audio::RenderFrame::from_master_clock(self.audio.monitor.output_frames())
            });
        self.recorder.log_at(
            frame,
            "set_fader_curve",
            serde_json::json!({ "curve": curve.as_str() }),
        );
    }

    fn resolve_xfader_gains(&self) -> Option<crate::audio::RenderFrame> {
        let position = self.audio.monitor.xfader_position();
        self.for_each_strip(|strip| strip.set_xfader_position(position))
    }

    /// The frame comes back from the same lock the write took, so a master-scope move is
    /// stamped where the strips render it rather than off the free-running clock. Taken as a
    /// max because `deck_ids` is unordered and "the last one locked" is arbitrary.
    fn for_each_strip(
        &self,
        mut write: impl FnMut(&mut ChannelStrip),
    ) -> Option<crate::audio::RenderFrame> {
        let mut frame = None;
        for deck in self.audio.deck_ids() {
            let Some(strip) = self.audio.strip(&deck) else {
                continue;
            };
            let mut strip = strip.locked();
            write(&mut strip);
            frame = Some(
                frame.map_or(strip.render_frame(), |seen: crate::audio::RenderFrame| {
                    if strip.render_frame().get() > seen.get() {
                        strip.render_frame()
                    } else {
                        seen
                    }
                }),
            );
        }
        frame
    }

    pub(crate) fn master_frame(&self) -> crate::audio::RenderFrame {
        self.for_each_strip(|_| {}).unwrap_or_else(|| {
            crate::audio::RenderFrame::from_master_clock(self.audio.monitor.output_frames())
        })
    }

    pub(crate) fn set_cue_active(
        &self,
        origin: ParamOrigin,
        deck: &str,
        active: bool,
    ) -> Result<(), String> {
        self.audio
            .strip(deck)
            .ok_or_else(|| format!("unknown deck: {}", deck))?
            .locked()
            .cue_active = active;
        self.apply_cue_active(origin, deck, active);
        Ok(())
    }

    /// Read and write under one lock: the controller sends a press, not a
    /// state, so the engine is the only thing that knows what it toggles from.
    pub(crate) fn toggle_cue_active(&self, origin: ParamOrigin, deck: &str) -> Result<(), String> {
        let strip = self
            .audio
            .strip(deck)
            .ok_or_else(|| format!("unknown deck: {}", deck))?;
        let active = {
            let mut guard = strip.locked();
            guard.cue_active = !guard.cue_active;
            guard.cue_active
        };
        self.apply_cue_active(origin, deck, active);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording_engine() -> Engine {
        let engine = Engine::for_testing(48_000);
        engine.recorder.start(
            engine.audio.mixer(),
            engine.audio.monitor.capture_start_handle(),
            crate::audio::RenderFrame::from_master_clock(0),
            [],
        );
        // Stands in for the tap: frames are dropped entirely until it says a buffer landed.
        engine
            .audio
            .monitor
            .capture_start_handle()
            .store(0, std::sync::atomic::Ordering::Relaxed);
        engine
    }

    fn logged_events(engine: Engine) -> Vec<serde_json::Value> {
        engine
            .recorder
            .stop(crate::audio::RenderFrame::from_master_clock(0));
        let pending = engine.recorder.take_pending().expect("a stopped recording");
        let parsed: serde_json::Value = serde_json::from_str(&pending).expect("valid JSON");
        parsed["events"].as_array().cloned().unwrap_or_default()
    }

    #[test]
    fn a_transport_press_logs_at_the_frame_the_deck_will_next_render() {
        let engine = recording_engine();
        engine
            .audio
            .deck("A")
            .expect("deck A")
            .locked()
            .set_next_render_frame(4096);

        engine
            .toggle_play(ParamOrigin::Ui, "A")
            .expect("deck A is a live deck");

        let events = logged_events(engine);
        let transport = events
            .iter()
            .find(|event| event["type"] == "play" || event["type"] == "stop")
            .expect("a transport event");
        assert_eq!(transport["frame"], 4096);
    }

    #[test]
    fn a_mixer_write_logs_at_the_frame_the_strip_will_next_render() {
        let engine = recording_engine();
        engine
            .audio
            .strip("A")
            .expect("strip A")
            .locked()
            .set_next_render_frame(2048);

        engine
            .set_deck_param(ParamOrigin::Ui, "A", "eq", "low", -6.0)
            .expect("eq/low is a classic-mixer address");

        let events = logged_events(engine);
        let param = events
            .iter()
            .find(|event| event["type"] == "set_param")
            .expect("a set_param event");
        assert_eq!(param["frame"], 2048);
        assert_eq!(param["value"], -6.0);
    }

    #[test]
    fn stopping_a_deck_outside_the_play_toggle_is_logged() {
        let engine = recording_engine();
        engine
            .audio
            .deck("A")
            .expect("deck A")
            .locked()
            .set_next_render_frame(8192);

        engine.stop("A").expect("deck A is a live deck");

        let events = logged_events(engine);
        let stop = events
            .iter()
            .find(|event| event["type"] == "stop")
            .expect("a stop event");
        assert_eq!(stop["frame"], 8192);
    }

    #[test]
    fn a_deck_the_mixer_does_not_have_is_refused() {
        let engine = Engine::for_testing(48_000);
        assert!(engine.toggle_play(ParamOrigin::Ui, "Z").is_err());
        assert!(engine
            .set_deck_param(ParamOrigin::Ui, "Z", "eq", "low", 0.0)
            .is_err());
    }
}

#[cfg(test)]
mod loop_and_quantize {
    use super::*;
    use crate::audio::CuePressOutcome;

    const SR: u32 = 44100;
    const SR_F: f64 = SR as f64;
    const BPM: f64 = 120.0;

    fn deck_with_grid(duration_secs: f64) -> Deck {
        let mut deck_state = Deck::loaded_for_testing(SR, duration_secs);
        deck_state.bpm = Some(BPM);
        deck_state.beat_offset_frames = 0.0;
        deck_state
    }

    #[test]
    fn quantize_to_beat_snaps_to_exact_beat_boundary() {
        let pos = beat_dur() * 4.0;
        let result = quantize_to_beat(pos, BPM, 0.0, SR_F);
        assert!((result - pos).abs() < 1.0, "got {}", result);
    }

    #[test]
    fn quantize_to_beat_rounds_to_nearest_beat() {
        let pos = beat_dur() * 2.3; // 30% into beat 2, closer to beat 2
        let result = quantize_to_beat(pos, BPM, 0.0, SR_F);
        let expected = beat_dur() * 2.0;
        assert!((result - expected).abs() < 1.0, "got {}", result);
    }

    #[test]
    fn quantize_to_beat_rounds_up_past_midpoint() {
        let pos = beat_dur() * 2.7; // 70% into beat 2, closer to beat 3
        let result = quantize_to_beat(pos, BPM, 0.0, SR_F);
        let expected = beat_dur() * 3.0;
        assert!((result - expected).abs() < 1.0, "got {}", result);
    }

    #[test]
    fn quantize_to_beat_never_returns_negative() {
        let result = quantize_to_beat(1.0, 120.0, 0.0, SR_F);
        assert!(result >= 0.0);
    }

    #[test]
    fn quantize_to_beat_respects_beat_offset() {
        let offset = beat_dur() * 0.25;
        let pos = offset + beat_dur();
        let result = quantize_to_beat(pos, BPM, offset, SR_F);
        assert!((result - pos).abs() < 1.0, "got {}", result);
    }

    fn press_cue_quantized(deck_state: &mut Deck) -> CuePressOutcome {
        if let Some(bpm) = deck_state.bpm {
            let sr = deck_state.device_sample_rate as f64;
            deck_state.main_pos =
                quantize_to_beat(deck_state.main_pos, bpm, deck_state.beat_offset_frames, sr);
        }
        deck_state.press_cue()
    }

    #[test]
    fn loop_in_returns_current_position_when_not_quantized() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.main_pos = beat_dur() * 2.7; // between beats
        let sec = loop_in_core(&mut deck_state, &mut ChannelStrip::new(SR_F as f32)).unwrap();
        assert!((sec - deck_state.main_pos / SR_F).abs() < 1e-9);
    }

    #[test]
    fn loop_in_snaps_to_beat_when_quantized() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = true;
        deck_state.main_pos = beat_dur() * 2.3; // 30% into beat 2 → should snap to beat 2
        let sec = loop_in_core(&mut deck_state, &mut ChannelStrip::new(SR_F as f32)).unwrap();
        let expected = 2.0 * beat_dur() / SR_F;
        assert!(
            (sec - expected).abs() < 1e-3,
            "expected ~{:.4} got {:.4}",
            expected,
            sec
        );
    }

    #[test]
    fn loop_in_updates_cue_point_to_playhead() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = 0.0;
        deck_state.main_pos = beat_dur() * 3.0;
        loop_in_core(&mut deck_state, &mut ChannelStrip::new(SR_F as f32)).unwrap();
        assert!(
            (deck_state.cue_point - beat_dur() * 3.0).abs() < 1e-9,
            "cue_point must move to playhead"
        );
    }

    #[test]
    fn loop_in_does_not_stop_playback() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.is_playing = true;
        deck_state.main_pos = beat_dur() * 3.0;
        loop_in_core(&mut deck_state, &mut ChannelStrip::new(SR_F as f32)).unwrap();
        assert!(deck_state.is_playing, "loop_in must not stop playback");
    }

    #[test]
    fn loop_in_clears_existing_loop_region() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = beat_dur();
        deck_state.loop_end = beat_dur() * 3.0;
        deck_state.loop_active = true;
        deck_state.main_pos = beat_dur() * 4.0;
        loop_in_core(&mut deck_state, &mut ChannelStrip::new(SR_F as f32)).unwrap();
        assert!(!deck_state.loop_active);
        assert!(
            (deck_state.cue_point - beat_dur() * 4.0).abs() < 1e-9,
            "cue_point updated to new loop-in position"
        );
        assert_eq!(deck_state.loop_end, 0.0);
    }

    #[test]
    fn loop_in_fails_without_beat_grid() {
        let mut deck_state = Deck::loaded_for_testing(SR, 10.0);
        deck_state.quantize = true;
        let result = loop_in_core(&mut deck_state, &mut ChannelStrip::new(SR_F as f32));
        assert!(result.is_err());
    }

    #[test]
    fn loop_out_creates_region_using_cue_point_as_start() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = beat_dur() * 2.0;
        deck_state.main_pos = beat_dur() * 4.0;
        let result = loop_out_core(&mut deck_state, &mut ChannelStrip::new(SR_F as f32))
            .unwrap()
            .unwrap();
        assert!((result.start_sec - beat_dur() * 2.0 / SR_F).abs() < 1e-6);
        assert!((result.end_sec - beat_dur() * 4.0 / SR_F).abs() < 1e-6);
        assert!(deck_state.loop_active);
    }

    #[test]
    fn loop_out_returns_none_when_cue_is_past_out() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = beat_dur() * 3.0;
        deck_state.main_pos = beat_dur() * 2.0; // playhead before cue → no loop
        let result = loop_out_core(&mut deck_state, &mut ChannelStrip::new(SR_F as f32)).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn loop_out_quantized_snaps_end_to_beat() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = true;
        deck_state.cue_point = 0.0;
        deck_state.main_pos = beat_dur() * 4.0 + beat_dur() * 0.1; // 10% past beat 4
        let result = loop_out_core(&mut deck_state, &mut ChannelStrip::new(SR_F as f32))
            .unwrap()
            .unwrap();
        let expected_end = beat_dur() * 4.0 / SR_F;
        assert!((result.end_sec - expected_end).abs() < 1e-3);
    }

    #[test]
    fn loop_out_quantized_compensates_late_press_position() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = true;
        let out_beat = beat_dur() * 4.0;
        deck_state.cue_point = 0.0;
        // Position is 5% past the loop end beat → late press
        deck_state.main_pos = out_beat + beat_dur() * 0.05;
        let result = loop_out_core(&mut deck_state, &mut ChannelStrip::new(SR_F as f32))
            .unwrap()
            .unwrap();
        assert!(
            result.seek_to_sec.is_some(),
            "expected seek compensation for late press"
        );
        let compensated = result.seek_to_sec.unwrap();
        assert!(compensated >= result.start_sec);
        assert!(compensated < result.end_sec);
        assert!((deck_state.main_pos - compensated * SR_F).abs() < 1.0);
    }

    #[test]
    fn loop_out_returns_none_when_cue_equals_out() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = 0.0;
        deck_state.main_pos = 0.0; // out == cue → zero-length, rejected
        let result = loop_out_core(&mut deck_state, &mut ChannelStrip::new(SR_F as f32)).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn loop_out_arms_only_when_playhead_is_strictly_after_cue() {
        let cue = beat_dur() * 2.0;

        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = cue;
        deck_state.main_pos = beat_dur() * 4.0;
        assert!(
            loop_out_core(&mut deck_state, &mut ChannelStrip::new(SR_F as f32))
                .unwrap()
                .is_some(),
            "playhead after cue: loop must arm"
        );
        assert!(deck_state.loop_active);

        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = cue;
        deck_state.main_pos = cue;
        assert!(
            loop_out_core(&mut deck_state, &mut ChannelStrip::new(SR_F as f32))
                .unwrap()
                .is_none(),
            "playhead at cue: must not arm"
        );
        assert!(!deck_state.loop_active);

        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = cue;
        deck_state.main_pos = beat_dur() * 1.0;
        assert!(
            loop_out_core(&mut deck_state, &mut ChannelStrip::new(SR_F as f32))
                .unwrap()
                .is_none(),
            "playhead before cue: must not arm"
        );
        assert!(!deck_state.loop_active);
    }

    #[test]
    fn loop_out_beat_count_matches_region_duration() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.quantize = false;
        deck_state.cue_point = 0.0;
        deck_state.main_pos = beat_dur() * 4.0;
        let result = loop_out_core(&mut deck_state, &mut ChannelStrip::new(SR_F as f32))
            .unwrap()
            .unwrap();
        assert_eq!(result.beats, 4);
    }

    #[test]
    fn press_cue_quantized_snaps_new_cue_to_nearest_beat() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.cue_point = 0.0;
        deck_state.main_pos = beat_dur() * 2.3; // closer to beat 2
        let outcome = press_cue_quantized(&mut deck_state);
        let expected = beat_dur() * 2.0;
        assert_eq!(
            outcome,
            CuePressOutcome::CueMoved {
                new_cue_point_sec: expected / SR_F
            }
        );
        assert!(
            (deck_state.cue_point - expected).abs() < 1.0,
            "cue_point must snap to nearest beat"
        );
        assert!(
            (deck_state.main_pos - expected).abs() < 1.0,
            "main_pos must also snap"
        );
    }

    #[test]
    fn press_cue_quantized_rounds_up_past_midpoint() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.cue_point = 0.0;
        deck_state.main_pos = beat_dur() * 2.7; // past midpoint → rounds to beat 3
        press_cue_quantized(&mut deck_state);
        let expected = beat_dur() * 3.0;
        assert!((deck_state.cue_point - expected).abs() < 1.0);
    }

    #[test]
    fn press_cue_quantized_moves_cue_clears_loop() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.cue_point = 0.0;
        deck_state.loop_end = beat_dur() * 4.0;
        deck_state.loop_active = true;
        deck_state.main_pos = beat_dur() * 2.0;
        let had_loop = deck_state.loop_end > 0.0;
        let out = press_cue_quantized(&mut deck_state);
        let loop_cleared = matches!(out, CuePressOutcome::CueMoved { .. }) && had_loop;
        if loop_cleared {
            deck_state.loop_active = false;
            deck_state.loop_end = 0.0;
        }
        assert!(loop_cleared, "moving the cue must clear the existing loop");
        assert!(!deck_state.loop_active);
        assert_eq!(deck_state.loop_end, 0.0);
    }

    #[test]
    fn press_cue_quantized_preview_at_cue_does_not_clear_loop() {
        let mut deck_state = deck_with_grid(10.0);
        deck_state.cue_point = beat_dur() * 2.0;
        deck_state.loop_end = beat_dur() * 4.0;
        deck_state.loop_active = true;
        deck_state.main_pos = deck_state.cue_point; // already at cue → preview, not move
        let had_loop = deck_state.loop_end > 0.0;
        let out = press_cue_quantized(&mut deck_state);
        let loop_cleared = matches!(out, CuePressOutcome::CueMoved { .. }) && had_loop;
        assert!(!loop_cleared, "preview at cue must not clear the loop");
        assert!(deck_state.loop_active);
    }

    /// The callback claims its buffer at the top and stamps each deck as it reaches it, so
    /// between those two moments the master clock already names the next buffer.
    fn engine_mid_callback() -> Engine {
        let engine = Engine::for_testing(48_000);
        engine.recorder.start(
            engine.audio.mixer(),
            engine.audio.monitor.capture_start_handle(),
            crate::audio::RenderFrame::from_master_clock(0),
            [],
        );
        engine
            .audio
            .monitor
            .capture_start_handle()
            .store(0, std::sync::atomic::Ordering::Relaxed);
        engine
            .audio
            .monitor
            .output_frames
            .store(8192, std::sync::atomic::Ordering::Relaxed);
        // Every deck the callback renders, the edit deck included, or the fixture leaves a
        // strip at zero and the assertion depends on unordered iteration.
        for id in engine.audio.deck_ids() {
            if let Some(deck) = engine.audio.deck(&id) {
                deck.locked().set_next_render_frame(4096);
            }
            if let Some(strip) = engine.audio.strip(&id) {
                strip.locked().set_next_render_frame(4096);
            }
        }
        engine
    }

    #[test]
    fn releasing_cue_returns_the_playhead_to_where_the_preview_began() {
        let engine = Engine::for_testing(48_000);
        let deck_arc = engine.deck("A").expect("deck A");
        {
            let mut deck = deck_arc.lock().unwrap();
            deck.samples = std::sync::Arc::new(vec![0.0; 48_000 * 2 * 30]);
            deck.channels = 2;
            deck.total_frames = 48_000 * 30;
            deck.bpm = Some(BPM);
            deck.quantize = false;
            deck.cue_point = beat_dur() * 4.0;
            deck.main_pos = beat_dur() * 4.0;
        }

        engine.press_cue(ParamOrigin::Ui, "A").expect("press");
        {
            let mut deck = deck_arc.lock().unwrap();
            assert!(deck.is_cueing, "the press must start a preview");
            deck.main_pos += 5_000.0;
        }

        let payload = engine.release_cue(ParamOrigin::Ui, "A").expect("release");
        let deck = deck_arc.lock().unwrap();
        assert!(!deck.is_cueing);
        assert!(!deck.is_playing);
        assert_eq!(deck.main_pos, beat_dur() * 4.0, "playhead");
        assert_eq!(
            payload.position_sec,
            beat_dur() * 4.0 / 48_000.0,
            "the payload the UI draws from"
        );
    }

    fn beat_dur() -> f64 {
        (60.0 / BPM) * SR_F
    }

    fn logged(engine: Engine, event_type: &str) -> serde_json::Value {
        engine
            .recorder
            .stop(crate::audio::RenderFrame::from_master_clock(0));
        let pending = engine.recorder.take_pending().expect("a stopped recording");
        let parsed: serde_json::Value = serde_json::from_str(&pending).expect("valid JSON");
        parsed["events"]
            .as_array()
            .expect("events")
            .iter()
            .find(|event| event["type"] == event_type)
            .cloned()
            .unwrap_or_else(|| panic!("no {event_type} event"))
    }

    #[test]
    fn a_seek_logs_the_frame_the_deck_will_render_at() {
        let engine = engine_mid_callback();
        engine.seek("A", 1.0).expect("deck A");
        assert_eq!(logged(engine, "seek")["frame"], 4096);
    }

    #[test]
    fn a_nudge_logs_the_frame_the_deck_will_render_at() {
        let engine = engine_mid_callback();
        engine.set_nudge("A", 2.0).expect("deck A");
        assert_eq!(logged(engine, "set_nudge")["frame"], 4096);
    }

    #[test]
    fn an_eject_logs_the_frame_the_deck_will_render_at() {
        let engine = engine_mid_callback();
        engine.eject_track("A").expect("deck A");
        assert_eq!(logged(engine, "eject_track")["frame"], 4096);
    }

    #[test]
    fn a_beat_grid_logs_the_frame_the_deck_will_render_at() {
        let engine = engine_mid_callback();
        engine.set_beat_grid("A", 128.0, 0.1).expect("deck A");
        assert_eq!(logged(engine, "set_beat_grid")["frame"], 4096);
    }

    #[test]
    fn a_fader_curve_logs_the_frame_the_strips_will_render_at() {
        let engine = engine_mid_callback();
        engine.set_fader_curve(session_core::FaderCurve::Linear);
        assert_eq!(logged(engine, "set_fader_curve")["frame"], 4096);
    }

    #[test]
    fn a_crossfader_move_logs_the_frame_the_strips_will_render_at() {
        let engine = engine_mid_callback();
        engine.set_xfader_position(ParamOrigin::Ui, 0.5);
        assert_eq!(logged(engine, "set_param")["frame"], 4096);
    }

    #[test]
    fn a_master_gain_move_logs_the_frame_the_strips_will_render_at() {
        let engine = engine_mid_callback();
        engine.set_master_gain(0.5);
        assert_eq!(logged(engine, "set_param")["frame"], 4096);
    }

    #[test]
    fn a_strip_reports_whether_an_address_reached_a_unit() {
        let mut strip = ChannelStrip::from_manifest(&session_core::CLASSIC_3BAND_V2, 48_000.0);
        assert!(strip.set_param("eq", "low", -6.0));
        assert!(!strip.set_param("eq", "presence", -6.0));
        assert!(!strip.set_param("reverb", "mix", 0.5));
    }

    #[test]
    fn an_address_no_unit_answers_is_refused_rather_than_logged() {
        let engine = engine_mid_callback();
        assert!(engine
            .set_deck_param(ParamOrigin::Ui, "A", "reverb", "mix", 0.5)
            .is_err());

        engine
            .recorder
            .stop(crate::audio::RenderFrame::from_master_clock(0));
        let pending = engine.recorder.take_pending().expect("a stopped recording");
        let parsed: serde_json::Value = serde_json::from_str(&pending).expect("valid JSON");
        let params: Vec<_> = parsed["events"]
            .as_array()
            .expect("events")
            .iter()
            .filter(|event| event["type"] == "set_param")
            .collect();
        assert!(params.is_empty(), "wrote {params:?} that no reader applies");
    }
}
