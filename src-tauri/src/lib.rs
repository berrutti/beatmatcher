mod audio;
mod broadcast;
mod commands;
mod engine_push;
mod midi;
pub mod offline_render;
mod session_playback;

use audio::AppAudio;
use commands::DeckSyncPayload;
use engine_push::{EnginePush, ParamOrigin};
use std::sync::Arc;

type TrackCache = Arc<std::sync::Mutex<session_playback::SampleCache>>;
type LedFeedback = Box<dyn Fn(midi::Feedback, &str, bool) + Send + Sync>;
use tauri::menu::{
    AboutMetadataBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::Emitter;
use tauri::Manager;

fn system_time_to_iso8601(system_time: std::time::SystemTime) -> String {
    const SECS_PER_DAY: i64 = 86400;
    const SECS_PER_HOUR: i64 = 3600;
    const SECS_PER_MIN: i64 = 60;
    // Offset from Unix epoch (1970-01-01) to the proleptic Gregorian epoch (0000-03-01).
    const DAYS_TO_GREGORIAN_EPOCH: i64 = 719468;
    const DAYS_PER_400_YEARS: i64 = 146097;
    const DAYS_PER_100_YEARS: i64 = 36524;
    const DAYS_PER_4_YEARS: i64 = 1460;
    const DAYS_PER_YEAR: i64 = 365;

    let elapsed = system_time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = elapsed.as_secs() as i64;
    let milliseconds = elapsed.subsec_millis();

    let time_of_day = total_secs.rem_euclid(SECS_PER_DAY);
    let hours = time_of_day / SECS_PER_HOUR;
    let minutes = (time_of_day % SECS_PER_HOUR) / SECS_PER_MIN;
    let seconds = time_of_day % SECS_PER_MIN;

    let days_since_gregorian_epoch = total_secs.div_euclid(SECS_PER_DAY) + DAYS_TO_GREGORIAN_EPOCH;
    let gregorian_era = days_since_gregorian_epoch.div_euclid(DAYS_PER_400_YEARS);
    let day_of_era = (days_since_gregorian_epoch - gregorian_era * DAYS_PER_400_YEARS) as u64;
    let year_of_era = (day_of_era - day_of_era / DAYS_PER_4_YEARS as u64
        + day_of_era / DAYS_PER_100_YEARS as u64
        - day_of_era / (DAYS_PER_400_YEARS - 1) as u64)
        / DAYS_PER_YEAR as u64;
    let year_raw = year_of_era as i64 + gregorian_era * 400;
    let day_of_year =
        day_of_era - (DAYS_PER_YEAR as u64 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_param = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_param + 2) / 5 + 1;
    let month = if month_param < 10 {
        month_param + 3
    } else {
        month_param - 9
    };
    let year = year_raw + if month <= 2 { 1 } else { 0 };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, month, day, hours, minutes, seconds, milliseconds
    )
}

struct SessionLogger {
    start: std::time::Instant,
    start_wall: std::time::SystemTime,
    mixer: &'static session_core::MixerManifest,
    events: Vec<serde_json::Value>,
    pending: Option<String>,
}

impl SessionLogger {
    fn new(mixer: &'static session_core::MixerManifest) -> Self {
        Self {
            start: std::time::Instant::now(),
            start_wall: std::time::SystemTime::now(),
            mixer,
            events: Vec::new(),
            pending: None,
        }
    }

    fn log(&mut self, event_type: &str, payload: serde_json::Value) {
        let elapsed_ms = self.start.elapsed().as_secs_f64() * 1000.0;
        let mut obj = serde_json::Map::new();
        obj.insert("elapsed_ms".into(), serde_json::json!(elapsed_ms));
        obj.insert("type".into(), serde_json::json!(event_type));
        if let serde_json::Value::Object(extra) = payload {
            obj.extend(extra);
        }
        self.events.push(serde_json::Value::Object(obj));
    }

    fn stop(&mut self) {
        let started_at = system_time_to_iso8601(self.start_wall);
        let log = serde_json::json!({
            "version": session_core::BMS_VERSION,
            "startedAt": started_at,
            "mixer": self.mixer.header(),
            "events": std::mem::take(&mut self.events),
        });
        self.pending = serde_json::to_string_pretty(&log).ok();
    }

    fn take_pending(&mut self) -> Option<String> {
        self.pending.take()
    }
}

pub struct AppState {
    pub audio: Arc<AppAudio>,
    pub session_playback_cancel: std::sync::Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>,
    pub session_playback_handle: std::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    pub session_track_cache: TrackCache,
    pub session_track_loads: session_playback::TrackLoads,
    pub decode_permits: Arc<tokio::sync::Semaphore>,
    pub session_snapshots: std::sync::Mutex<
        std::collections::HashMap<String, Vec<crate::session_playback::SessionSnapshot>>,
    >,
    // In-memory source of truth for loaded sessions, keyed by .bms path. Holds
    // unsaved edits pushed from the frontend; playback and offline render read
    // from here so edits are audible before the file is written.
    pub session_files: std::sync::Mutex<
        std::collections::HashMap<String, Arc<crate::offline_render::SessionFile>>,
    >,
    session: std::sync::Mutex<Option<SessionLogger>>,
    // Mirrored from the frontend, which owns mode. Rust needs it only because
    // the MIDI thread has no other way to know the session scheduler is running.
    app_mode: std::sync::Mutex<AppMode>,
    pub engine_push: Arc<EnginePush>,
    // Set once at startup, the reverse of the MIDI dispatch closure: this lets the engine
    // light the controller's buttons without `AppState` knowing about `MidiState`.
    led_feedback: std::sync::Mutex<Option<LedFeedback>>,
}

/// Matches the pitch slider's `step`. Rate has no descriptor, so nothing else
/// quantizes it and one 14-bit sweep would log thousands of events.
const PITCH_STEPS_PER_PERCENT: f64 = 100.0;

const MIN_PLAYBACK_RATE: f64 = 0.1;

fn rate_from_fader(position: f64, pitch_range_percent: f64) -> f64 {
    let offset_percent = pitch_range_percent * (position.clamp(0.0, 1.0) * 2.0 - 1.0);
    let stepped = (offset_percent * PITCH_STEPS_PER_PERCENT).round() / PITCH_STEPS_PER_PERCENT;
    1.0 + stepped / 100.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppMode {
    Performance,
    Edit,
    Session,
}

impl AppState {
    pub(crate) fn app_mode(&self) -> AppMode {
        *self
            .app_mode
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    pub(crate) fn set_app_mode(&self, mode: AppMode) {
        *self
            .app_mode
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = mode;
    }

    fn log(&self, event_type: &str, payload: serde_json::Value) {
        if let Some(logger) = self
            .session
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_mut()
        {
            logger.log(event_type, payload);
        }
    }

    fn log_param(&self, deck: Option<&str>, slot: &str, param: &str, value: f64) {
        let mut payload = serde_json::Map::new();
        if let Some(deck) = deck {
            payload.insert("deck".into(), serde_json::json!(deck));
        }
        payload.insert("slot".into(), serde_json::json!(slot));
        payload.insert("param".into(), serde_json::json!(param));
        payload.insert("value".into(), serde_json::json!(value));
        self.log("set_param", serde_json::Value::Object(payload));
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
        {
            let mut strip = strip_arc.lock().unwrap_or_else(|error| error.into_inner());
            // A 14-bit control resolves a move on each half, so the same value arrives twice per
            // physical move. Logging both would write an event nothing can hear.
            if strip.param(slot, param) == Some(value) {
                return Ok(());
            }
            strip.set_param(slot, param, value);
        }
        self.log_param(Some(deck), slot, param, value as f64);
        self.engine_push.mark(origin, deck, slot, param);
        Ok(())
    }

    pub(crate) fn deck(
        &self,
        deck: &str,
    ) -> Result<Arc<std::sync::Mutex<audio::DeckState>>, String> {
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
        let deck_arc = self.deck(deck)?;
        let payload = {
            let mut deck_state = deck_arc.lock().unwrap_or_else(|e| e.into_inner());
            deck_state.toggle_play();
            DeckSyncPayload::from_deck(&deck_state, false)
        };
        self.log(
            if payload.is_playing { "play" } else { "stop" },
            serde_json::json!({ "deck": deck }),
        );
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
        let (outcome, payload) = {
            let mut deck_state = deck_arc.lock().unwrap_or_else(|e| e.into_inner());
            if deck_state.quantize {
                if let Some(bpm) = deck_state.bpm {
                    let sr = deck_state.device_sample_rate as f64;
                    deck_state.main_pos = commands::quantize_to_beat(
                        deck_state.main_pos,
                        bpm,
                        deck_state.beat_offset_frames,
                        sr,
                    );
                }
            }
            let had_loop = deck_state.loop_end > 0.0;
            let out = deck_state.press_cue();
            let loop_cleared = matches!(out, audio::CuePressOutcome::CueMoved { .. }) && had_loop;
            if loop_cleared {
                deck_state.loop_active = false;
                deck_state.loop_end = 0.0;
            }
            (out, DeckSyncPayload::from_deck(&deck_state, loop_cleared))
        };
        self.engine_push
            .mark_transport(origin, deck, payload.loop_region_cleared);
        let (event, cue_sec) = match outcome {
            audio::CuePressOutcome::NoTrack => return Ok(payload),
            audio::CuePressOutcome::PreviewStarted => ("cue_preview_start", payload.cue_point_sec),
            audio::CuePressOutcome::CueMoved { new_cue_point_sec } => {
                ("cue_move", new_cue_point_sec)
            }
            audio::CuePressOutcome::StoppedAtCue { cue_point_sec } => {
                ("stopped_at_cue", cue_point_sec)
            }
        };
        self.log(
            event,
            serde_json::json!({ "deck": deck, "cue_point_sec": cue_sec }),
        );
        Ok(payload)
    }

    pub(crate) fn release_cue(
        &self,
        origin: ParamOrigin,
        deck: &str,
    ) -> Result<DeckSyncPayload, String> {
        let deck_arc = self.deck(deck)?;
        let (was_cueing, payload) = {
            let mut deck_state = deck_arc.lock().unwrap_or_else(|e| e.into_inner());
            let was = deck_state.is_cueing;
            deck_state.release_cue();
            (was, DeckSyncPayload::from_deck(&deck_state, false))
        };
        if was_cueing {
            self.log(
                "cue_preview_end",
                serde_json::json!({ "deck": deck, "cue_point_sec": payload.cue_point_sec }),
            );
        }
        self.engine_push
            .mark_transport(origin, deck, payload.loop_region_cleared);
        Ok(payload)
    }

    pub(crate) fn set_cue_and_stop(
        &self,
        origin: ParamOrigin,
        deck: &str,
    ) -> Result<DeckSyncPayload, String> {
        let deck_arc = self.deck(deck)?;
        let (was_playing, payload) = {
            let mut deck_state = deck_arc.lock().unwrap_or_else(|e| e.into_inner());
            let was = deck_state.is_playing;
            deck_state.set_cue_and_stop();
            (was, DeckSyncPayload::from_deck(&deck_state, false))
        };
        if was_playing {
            self.log(
                "cue_set_and_stop",
                serde_json::json!({ "deck": deck, "cue_point_sec": payload.cue_point_sec }),
            );
        }
        self.engine_push
            .mark_transport(origin, deck, payload.loop_region_cleared);
        Ok(payload)
    }

    pub(crate) fn stop_at_cue(
        &self,
        origin: ParamOrigin,
        deck: &str,
    ) -> Result<DeckSyncPayload, String> {
        let deck_arc = self.deck(deck)?;
        let (was_playing, payload) = {
            let mut deck_state = deck_arc.lock().unwrap_or_else(|e| e.into_inner());
            let was = deck_state.is_playing;
            deck_state.stop_at_cue();
            (was, DeckSyncPayload::from_deck(&deck_state, false))
        };
        if was_playing {
            self.log(
                "stop_at_cue",
                serde_json::json!({ "deck": deck, "cue_point_sec": payload.cue_point_sec }),
            );
        }
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
        self.deck(deck)?
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .playback_rate = rate.max(MIN_PLAYBACK_RATE);
        self.log(
            "set_playback_rate",
            serde_json::json!({ "deck": deck, "rate": rate }),
        );
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
        if self
            .deck(deck)?
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .playback_rate
            == rate
        {
            return Ok(());
        }
        self.set_playback_rate(origin, deck, rate)
    }

    /// Wheel ticks accumulate on the deck and the audio thread consumes them, so this waits
    /// for no block. The transport mark makes the next flush read whatever the wheel moved.
    pub(crate) fn jog(&self, origin: ParamOrigin, deck: &str, ticks: i32) -> Result<(), String> {
        // Shift is scaled here rather than at consume time so the logged ticks are exactly
        // the ones the engine acts on, and a replay needs no shift state of its own.
        let scaled = {
            let deck_arc = self.deck(deck)?;
            let mut deck_state = deck_arc.lock().unwrap_or_else(|e| e.into_inner());
            let scaled = crate::audio::logged_jog_ticks(
                f64::from(ticks),
                deck_state.jog_shift,
                deck_state.is_playing,
            );
            deck_state.jog_pending += scaled;
            scaled
        };
        self.log(
            "jog",
            serde_json::json!({ "deck": deck, "ticks": scaled }),
        );
        self.engine_push.mark_transport(origin, deck, false);
        Ok(())
    }

    /// Held on the surface rather than latched, so it is set on both edges and
    /// moves nothing on its own.
    pub(crate) fn set_jog_shift(&self, deck: &str, held: bool) -> Result<(), String> {
        self.deck(deck)?
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_jog_shift(held);
        Ok(())
    }

    pub(crate) fn loop_in(
        &self,
        origin: ParamOrigin,
        deck: &str,
    ) -> Result<DeckSyncPayload, String> {
        let deck_arc = self.deck(deck)?;
        let (payload, cue_sec, quantize) = {
            let mut deck_state = deck_arc.lock().unwrap_or_else(|e| e.into_inner());
            let sec = commands::loop_in_core(&mut deck_state)?;
            let payload = DeckSyncPayload::from_deck(&deck_state, true);
            (payload, sec, deck_state.quantize)
        };
        self.log(
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
    ) -> Result<Option<commands::LoopOutResult>, String> {
        let deck_arc = self.deck(deck)?;
        let (result, quantize) = {
            let mut deck_state = deck_arc.lock().unwrap_or_else(|e| e.into_inner());
            let quantize = deck_state.quantize;
            (commands::loop_out_core(&mut deck_state)?, quantize)
        };
        if let Some(region) = &result {
            self.log(
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
        let deck_arc = self.deck(deck)?;
        let payload = {
            let mut deck_state = deck_arc.lock().unwrap_or_else(|e| e.into_inner());
            deck_state.loop_active = active;
            DeckSyncPayload::from_deck(&deck_state, false)
        };
        if !active {
            self.log("exit_loop", serde_json::json!({ "deck": deck }));
        }
        self.engine_push.mark_transport(origin, deck, false);
        Ok(payload)
    }

    pub(crate) fn reloop(
        &self,
        origin: ParamOrigin,
        deck: &str,
    ) -> Result<DeckSyncPayload, String> {
        let deck_arc = self.deck(deck)?;
        let payload = {
            let mut deck_state = deck_arc.lock().unwrap_or_else(|e| e.into_inner());
            if deck_state.loop_end > deck_state.cue_point {
                deck_state.main_pos = deck_state.cue_point;
                deck_state.cue_pos = deck_state.cue_point;
                if deck_state.is_playing {
                    deck_state.loop_active = true;
                }
            }
            DeckSyncPayload::from_deck(&deck_state, false)
        };
        self.log("reloop", serde_json::json!({ "deck": deck }));
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
            let deck_state = deck_arc.lock().unwrap_or_else(|e| e.into_inner());
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
        let deck_state = deck_arc.lock().unwrap_or_else(|e| e.into_inner());
        Ok(DeckSyncPayload::from_deck(&deck_state, false))
    }

    /// Cue is headphone routing rather than a mixer move, so it skips `set_deck_param`.
    /// It is still logged, under its own event type.
    fn apply_cue_active(&self, origin: ParamOrigin, deck: &str, active: bool) {
        self.log(
            "set_cue_active",
            serde_json::json!({ "deck": deck, "active": active }),
        );
        self.engine_push.mark_cue(origin, deck);
        self.light(midi::Feedback::Cue, deck, active);
    }

    /// Every writer of a lit control goes through here whatever moved it, so a
    /// mouse click lights the button the same as the button does.
    pub(crate) fn light(&self, kind: midi::Feedback, deck: &str, on: bool) {
        if let Some(feedback) = self
            .led_feedback
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            feedback(kind, deck, on);
        }
    }

    /// Position is one master value, but the gain it implies is per strip, so
    /// every strip is re-resolved against its own assign on each move.
    pub(crate) fn set_xfader_position(&self, origin: ParamOrigin, position: f32) {
        // A 14-bit control resolves a move on each half, so one sweep would otherwise
        // re-resolve every strip and log an event thousands of times over.
        if !self.audio.monitor.set_xfader_position(position) {
            return;
        }
        self.resolve_xfader_gains();
        let landed = self.audio.monitor.xfader_position();
        self.log_param(None, "xfader", "position", landed as f64);
        self.engine_push.mark_xfader(origin);
    }

    pub(crate) fn set_xfader_assign(
        &self,
        origin: ParamOrigin,
        deck: &str,
        assign: session_core::XfaderAssign,
    ) -> Result<(), String> {
        self.audio
            .strip(deck)
            .ok_or_else(|| format!("unknown deck: {}", deck))?
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .set_xfader_assign(assign);
        self.log(
            "set_xfader_assign",
            serde_json::json!({ "deck": deck, "assign": assign.as_str() }),
        );
        self.engine_push.mark_xfader_assign(origin, deck);
        Ok(())
    }

    /// One switch, every channel, as on a mixer: the strips each hold their own
    /// copy because the taper is applied before the fader's own smoothing.
    pub(crate) fn set_fader_curve(&self, curve: session_core::FaderCurve) {
        self.audio.monitor.set_fader_curve(curve);
        for deck in self.audio.deck_ids() {
            let Some(strip) = self.audio.strip(&deck) else {
                continue;
            };
            strip
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .set_fader_curve(curve);
        }
        self.log(
            "set_fader_curve",
            serde_json::json!({ "curve": curve.as_str() }),
        );
    }

    fn resolve_xfader_gains(&self) {
        let position = self.audio.monitor.xfader_position();
        for deck in self.audio.deck_ids() {
            let Some(strip) = self.audio.strip(&deck) else {
                continue;
            };
            strip
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .set_xfader_position(position);
        }
    }

    pub(crate) fn set_led_feedback(&self, feedback: LedFeedback) {
        *self
            .led_feedback
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(feedback);
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
            .lock()
            .unwrap_or_else(|error| error.into_inner())
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
            let mut guard = strip.lock().unwrap_or_else(|error| error.into_inner());
            guard.cue_active = !guard.cue_active;
            guard.cue_active
        };
        self.apply_cue_active(origin, deck, active);
        Ok(())
    }
}

// Required because AppState contains AppAudio, which contains SendStream.
// See stream.rs for the full safety argument.
unsafe impl Send for AppState {}
unsafe impl Sync for AppState {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let verbose = std::env::args().any(|arg| arg == "--verbose")
        || std::env::var("BEATMATCHER_VERBOSE").is_ok();
    let app_level = if verbose {
        log::LevelFilter::Info
    } else {
        log::LevelFilter::Warn
    };

    let audio = AppAudio::new().expect("failed to initialize audio engine");
    let ended_flags: Vec<(String, Arc<std::sync::atomic::AtomicBool>)> = audio
        .ended_flags
        .iter()
        .map(|(deck_id, flag)| (deck_id.clone(), flag.clone()))
        .collect();
    let app_state = AppState {
        audio: Arc::new(audio),
        session: std::sync::Mutex::new(None),
        session_playback_cancel: std::sync::Mutex::new(None),
        session_playback_handle: std::sync::Mutex::new(None),
        session_track_cache: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        session_track_loads: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        decode_permits: Arc::new(tokio::sync::Semaphore::new(
            std::thread::available_parallelism()
                .map(|cores| cores.get())
                .unwrap_or(4),
        )),
        session_snapshots: std::sync::Mutex::new(std::collections::HashMap::new()),
        session_files: std::sync::Mutex::new(std::collections::HashMap::new()),
        app_mode: std::sync::Mutex::new(AppMode::Performance),
        engine_push: Arc::new(EnginePush::new()),
        led_feedback: std::sync::Mutex::new(None),
    };

    // Cloned before `.manage()` consumes app_state, so the broadcaster thread
    // can read every deck's live state (same reason ended_flags is cloned above).
    let audio_for_broadcast = Arc::clone(&app_state.audio);
    let audio_for_push = Arc::clone(&app_state.audio);
    let engine_push = Arc::clone(&app_state.engine_push);

    tauri::Builder::default()
        .manage(app_state)
        .manage(midi::MidiState::new())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(move |app| {
            let icon = app.default_window_icon().cloned();
            let about = AboutMetadataBuilder::new()
                .name(Some("Beatmatcher"))
                .copyright(Some(
                    "Copyright 2026 Matias Berrutti\ngithub.com/berrutti/beatmatcher",
                ))
                .icon(icon)
                .build();
            let quit_item = MenuItemBuilder::new("Quit Beatmatcher")
                .accelerator("CmdOrCtrl+Q")
                .build(app)?;
            let quit_id = quit_item.id().clone();
            let app_menu = SubmenuBuilder::new(app, "Beatmatcher")
                .item(&PredefinedMenuItem::about(app, None, Some(about))?)
                .separator()
                .item(&PredefinedMenuItem::hide(app, None)?)
                .item(&PredefinedMenuItem::hide_others(app, None)?)
                .separator()
                .item(&quit_item)
                .build()?;
            let menu = MenuBuilder::new(app).item(&app_menu).build()?;
            app.set_menu(menu)?;
            app.on_menu_event(move |app, event| {
                if event.id() == &quit_id {
                    app.emit("quit-requested", ()).ok();
                }
            });

            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(app_level)
                    .level_for("symphonia_format_riff", log::LevelFilter::Warn)
                    .level_for("symphonia_format_isomp4", log::LevelFilter::Warn)
                    .level_for("symphonia_metadata", log::LevelFilter::Warn)
                    .level_for("symphonia_bundle_mp3", log::LevelFilter::Warn)
                    .build(),
            )?;
            let handle = app.handle().clone();
            let flags = ended_flags;
            tauri::async_runtime::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(100));
                loop {
                    interval.tick().await;
                    for (id, flag) in &flags {
                        if flag.swap(false, std::sync::atomic::Ordering::AcqRel) {
                            handle.emit("track-ended", id.clone()).ok();
                        }
                    }
                }
            });

            match app.path().app_data_dir() {
                Ok(data_dir) => broadcast::start(data_dir, audio_for_broadcast),
                Err(error) => log::warn!("performer broadcast disabled: {error}"),
            }

            engine_push::start(app.handle().clone(), audio_for_push, engine_push);

            midi::start_monitor(app.handle().clone(), &app.state::<midi::MidiState>());
            let midi_handle = app.handle().clone();
            midi::set_dispatch(
                &app.state::<midi::MidiState>(),
                Arc::new(move |port: &str, data: &[u8]| {
                    midi::apply(
                        midi_handle.state::<AppState>().inner(),
                        midi_handle.state::<midi::MidiState>().inner(),
                        &midi_handle,
                        port,
                        data,
                    );
                }),
            );
            let feedback_handle = app.handle().clone();
            app.state::<AppState>().set_led_feedback(Box::new(
                move |kind: midi::Feedback, deck: &str, active: bool| {
                    midi::send_led(
                        &feedback_handle.state::<midi::MidiState>(),
                        kind,
                        deck,
                        active,
                    );
                },
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            confirm_quit,
            commands::analyze_track,
            commands::clear_loop_region,
            commands::discard_recording,
            commands::render_session_to_file,
            commands::save_bms_only,
            commands::eject_track,
            commands::files_info,
            commands::get_deck_levels,
            commands::get_master_level,
            commands::get_spectral_waveform_region,
            commands::get_track_amplitude_waveform,
            commands::get_track_amplitude_region,
            commands::list_audio_devices,
            commands::load_track,
            midi::list_midi_devices,
            midi::set_midi_device_deck,
            midi::set_midi_monitor,
            commands::pick_save_path,
            commands::play,
            commands::press_cue,
            commands::read_file,
            commands::read_track_tags,
            commands::release_cue,
            commands::save_recording,
            commands::save_session,
            commands::scan_folder,
            commands::seek,
            commands::set_beat_grid,
            commands::set_bpm_range,
            commands::set_buffer_size,
            commands::set_app_mode,
            commands::set_pitch_range,
            commands::set_jog_rotation_speed,
            commands::set_fader_curve,
            commands::set_xfader_position,
            commands::set_xfader_assign,
            commands::set_cue_active,
            commands::set_cue_and_stop,
            commands::set_cue_device,
            commands::set_cue_mix,
            commands::set_deck_param,
            commands::set_limiter_enabled,
            commands::set_loop_active,
            commands::set_loop_in,
            commands::set_loop_out,
            commands::set_loop_region,
            commands::set_main_device,
            commands::set_master_gain,
            commands::set_nudge,
            commands::set_playback_rate,
            commands::set_quantize,
            commands::set_reloop,
            commands::set_deck_muted,
            commands::start_recording,
            commands::stop_at_cue,
            commands::stop_recording,
            commands::stop,
            commands::toggle_play,
            session_playback::open_session_dialog,
            session_playback::preload_session,
            session_playback::start_session_playback,
            session_playback::stop_session_playback,
            session_playback::unload_session,
            session_playback::update_session_events,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
fn confirm_quit(app: tauri::AppHandle) {
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    // A fader resting at its detent must play at exactly 1.0: a rate of 0.99999
    // is an out-of-tune deck that looks in tune.
    #[test]
    fn a_centred_tempo_fader_is_exactly_unity() {
        for range in [6.0, 8.0, 10.0, 16.0, 50.0, 100.0] {
            assert_eq!(rate_from_fader(0.5, range), 1.0);
        }
    }

    #[test]
    fn the_tempo_fader_ends_span_the_pitch_range() {
        assert_eq!(rate_from_fader(0.0, 10.0), 0.9);
        assert_eq!(rate_from_fader(1.0, 10.0), 1.1);
        assert_eq!(rate_from_fader(0.0, 100.0), 0.0);
        assert_eq!(rate_from_fader(1.0, 100.0), 2.0);
    }

    #[test]
    fn a_fader_position_outside_the_unit_interval_cannot_widen_the_range() {
        assert_eq!(rate_from_fader(-0.5, 10.0), 0.9);
        assert_eq!(rate_from_fader(2.0, 10.0), 1.1);
    }

    #[test]
    fn a_fader_sweep_cannot_out_resolve_the_pitch_slider() {
        const FOURTEEN_BIT_POSITIONS: i32 = 16384;
        let range = 10.0;
        let mut previous = f64::NAN;
        let mut distinct = 0;
        for step in 0..FOURTEEN_BIT_POSITIONS {
            let position = f64::from(step) / f64::from(FOURTEEN_BIT_POSITIONS - 1);
            let rate = rate_from_fader(position, range);
            if rate != previous {
                distinct += 1;
                previous = rate;
            }
        }
        // What the slider itself offers: -10.00 to +10.00 inclusive, in steps of 0.01.
        assert_eq!(distinct, 2001);
    }

    fn unix_secs(secs: u64) -> std::time::SystemTime {
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs)
    }

    fn unix_millis(ms: u64) -> std::time::SystemTime {
        std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms)
    }

    #[test]
    fn iso8601_unix_epoch_is_midnight_1970() {
        assert_eq!(
            system_time_to_iso8601(unix_secs(0)),
            "1970-01-01T00:00:00.000Z"
        );
    }

    #[test]
    fn iso8601_known_timestamp() {
        // 2026-05-19T12:34:56Z = 1_779_194_096 seconds since epoch
        assert_eq!(
            system_time_to_iso8601(unix_secs(1_779_194_096)),
            "2026-05-19T12:34:56.000Z"
        );
    }

    #[test]
    fn iso8601_milliseconds_preserved() {
        // 1_000 ms = 1 second; add 123 ms
        assert_eq!(
            system_time_to_iso8601(unix_millis(1_000_123)),
            "1970-01-01T00:16:40.123Z"
        );
    }

    #[test]
    fn iso8601_leap_year_feb_29() {
        // 2000-02-29T00:00:00Z = 951_782_400
        assert_eq!(
            system_time_to_iso8601(unix_secs(951_782_400)),
            "2000-02-29T00:00:00.000Z"
        );
    }

    #[test]
    fn iso8601_end_of_year() {
        // 1999-12-31T23:59:59Z = 946_684_799
        assert_eq!(
            system_time_to_iso8601(unix_secs(946_684_799)),
            "1999-12-31T23:59:59.000Z"
        );
    }

    #[test]
    fn session_logger_records_events_in_order() {
        let mut logger = SessionLogger::new(&session_core::CLASSIC_3BAND);
        logger.log("first", serde_json::json!({}));
        logger.log("second", serde_json::json!({}));
        logger.log("third", serde_json::json!({}));
        assert_eq!(logger.events.len(), 3);
        assert_eq!(logger.events[0]["type"], "first");
        assert_eq!(logger.events[1]["type"], "second");
        assert_eq!(logger.events[2]["type"], "third");
    }

    #[test]
    fn session_logger_merges_payload_fields() {
        let mut logger = SessionLogger::new(&session_core::CLASSIC_3BAND);
        logger.log("play", serde_json::json!({ "deck": "A", "rate": 1.0 }));
        let event = &logger.events[0];
        assert_eq!(event["type"], "play");
        assert_eq!(event["deck"], "A");
        assert_eq!(event["rate"], 1.0);
    }

    #[test]
    fn session_logger_timestamps_are_non_negative() {
        let mut logger = SessionLogger::new(&session_core::CLASSIC_3BAND);
        logger.log("e", serde_json::json!({}));
        let elapsed_ms = logger.events[0]["elapsed_ms"].as_f64().unwrap();
        assert!(elapsed_ms >= 0.0);
    }

    #[test]
    fn session_logger_stop_produces_valid_json() {
        let mut logger = SessionLogger::new(&session_core::CLASSIC_3BAND);
        logger.log("recording_start", serde_json::json!({}));
        logger.stop();
        let pending = logger
            .take_pending()
            .expect("stop should produce pending JSON");
        let parsed: serde_json::Value = serde_json::from_str(&pending).expect("valid JSON");
        assert_eq!(parsed["version"], 2);
        assert!(parsed["startedAt"].as_str().is_some());
        assert!(parsed["events"].is_array());
        assert_eq!(parsed["events"].as_array().unwrap().len(), 1);
    }

    // A recording has to name the mixer it was played on, or the renderer has
    // nothing to check against and falls back to assuming the classic one.
    #[test]
    fn session_logger_stamps_the_mixer_it_recorded_on() {
        let mut logger = SessionLogger::new(&session_core::CLASSIC_3BAND);
        logger.stop();
        let parsed: serde_json::Value =
            serde_json::from_str(&logger.take_pending().expect("pending")).expect("valid JSON");
        assert_eq!(parsed["mixer"]["id"], "classic-3band");
        assert_eq!(
            parsed["mixer"]["hash"],
            session_core::CLASSIC_3BAND.content_hash()
        );

        let round_tripped: session_core::SessionFile =
            serde_json::from_value(parsed).expect("parses back as a session");
        assert!(session_core::resolve_manifest(round_tripped.mixer.as_ref()).is_ok());
    }

    // Stamping a constant would pass the test above while mislabelling any
    // recording made on a mixer other than the one that constant names.
    #[test]
    fn session_logger_stamps_the_mixer_it_was_given() {
        let mut logger = SessionLogger::new(&session_core::ISOLATOR_3BAND);
        logger.stop();
        let parsed: serde_json::Value =
            serde_json::from_str(&logger.take_pending().expect("pending")).expect("valid JSON");
        assert_eq!(parsed["mixer"]["id"], "isolator-3band");
        assert_eq!(
            parsed["mixer"]["hash"],
            session_core::ISOLATOR_3BAND.content_hash()
        );
    }

    #[test]
    fn session_logger_take_pending_clears_state() {
        let mut logger = SessionLogger::new(&session_core::CLASSIC_3BAND);
        logger.stop();
        assert!(logger.take_pending().is_some());
        assert!(logger.take_pending().is_none());
    }

    #[test]
    fn session_logger_stop_clears_events() {
        let mut logger = SessionLogger::new(&session_core::CLASSIC_3BAND);
        logger.log("a", serde_json::json!({}));
        logger.log("b", serde_json::json!({}));
        logger.stop();
        assert!(logger.events.is_empty());
    }
}
