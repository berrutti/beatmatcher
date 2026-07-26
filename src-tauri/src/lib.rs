mod audio;
mod broadcast;
mod commands;
mod engine_push;
mod midi;
pub mod offline_render;
mod session_playback;

use audio::AppAudio;
use engine_push::{EnginePush, ParamOrigin};
use std::sync::Arc;

type TrackCache = std::sync::Mutex<session_playback::SampleCache>;
type CueFeedback = Box<dyn Fn(&str, bool) + Send + Sync>;
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
            "version": 1,
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
    pub engine_push: Arc<EnginePush>,
    // Set once at startup. The reverse of the MIDI dispatch closure: that one
    // lets the controller reach the engine, this one lets the engine light the
    // controller's buttons without `AppState` knowing about `MidiState`.
    cue_feedback: std::sync::Mutex<Option<CueFeedback>>,
}

impl AppState {
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

    /// The one path a deck param changes through, whoever moved it. Logging from
    /// here is what puts a MIDI move in the `.bms` on the same terms as a mouse
    /// move, and what stops a second control path from bypassing the log.
    pub(crate) fn set_deck_param(
        &self,
        origin: ParamOrigin,
        deck: &str,
        slot: &str,
        param: &str,
        value: f32,
    ) -> Result<(), String> {
        self.audio
            .strip(deck)
            .ok_or_else(|| format!("unknown deck: {}", deck))?
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .set_param(slot, param, value);
        self.log_param(Some(deck), slot, param, value as f64);
        self.engine_push.mark(origin, deck, slot, param);
        Ok(())
    }

    /// Cue is headphone routing rather than a mixer move, so it is not a
    /// manifest param and does not go through `set_deck_param`. It is still
    /// logged, under its own event type.
    fn apply_cue_active(&self, origin: ParamOrigin, deck: &str, active: bool) {
        self.log(
            "set_cue_active",
            serde_json::json!({ "deck": deck, "active": active }),
        );
        self.engine_push.mark_cue(origin, deck);
        if let Some(feedback) = self
            .cue_feedback
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            feedback(deck, active);
        }
    }

    pub(crate) fn set_cue_feedback(&self, feedback: CueFeedback) {
        *self
            .cue_feedback
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
        session_track_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        session_snapshots: std::sync::Mutex::new(std::collections::HashMap::new()),
        session_files: std::sync::Mutex::new(std::collections::HashMap::new()),
        engine_push: Arc::new(EnginePush::new()),
        cue_feedback: std::sync::Mutex::new(None),
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
                Arc::new(move |data: &[u8]| {
                    midi::apply(midi_handle.state::<AppState>().inner(), data);
                }),
            );
            let feedback_handle = app.handle().clone();
            app.state::<AppState>()
                .set_cue_feedback(Box::new(move |deck: &str, active: bool| {
                    midi::send_cue_led(&feedback_handle.state::<midi::MidiState>(), deck, active);
                }));
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
            midi::get_midi_input,
            midi::list_midi_inputs,
            midi::set_midi_input,
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
            commands::set_cue_active,
            commands::set_cue_and_stop,
            commands::set_cue_device,
            commands::set_cue_mix,
            commands::set_eq,
            commands::set_filter_active,
            commands::set_filter,
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
            commands::set_volume,
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
        assert_eq!(parsed["version"], 1);
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
