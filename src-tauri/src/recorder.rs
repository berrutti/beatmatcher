use crate::audio::{self, RenderFrame};
use crate::lock::LockIgnoringPoison;
use std::sync::{Arc, Mutex};

fn system_time_to_iso8601(system_time: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(system_time)
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

/// A widened f32 writes the noise tail of a value the mixer never had. `Display` does not.
pub(crate) fn f32_json(value: f32) -> serde_json::Value {
    match format!("{value}").parse::<f64>() {
        Ok(shortest) => serde_json::json!(shortest),
        Err(_) => serde_json::json!(value),
    }
}

struct SessionLogger {
    start: std::time::Instant,
    start_wall: std::time::SystemTime,
    mixer: &'static session_core::MixerManifest,
    events: Vec<serde_json::Value>,
    pending: Option<String>,
    capture_start: Arc<std::sync::atomic::AtomicU64>,
}

impl SessionLogger {
    fn new(
        mixer: &'static session_core::MixerManifest,
        capture_start: Arc<std::sync::atomic::AtomicU64>,
    ) -> Self {
        Self {
            start: std::time::Instant::now(),
            start_wall: std::time::SystemTime::now(),
            mixer,
            events: Vec::new(),
            pending: None,
            capture_start,
        }
    }

    fn log_at(&mut self, frame: u64, event_type: &str, payload: serde_json::Value) {
        let elapsed_ms = self.start.elapsed().as_secs_f64() * 1000.0;
        let mut obj = serde_json::Map::new();
        obj.insert("elapsed_ms".into(), serde_json::json!(elapsed_ms));
        obj.insert("type".into(), serde_json::json!(event_type));
        obj.insert("frame".into(), serde_json::json!(frame));
        if let serde_json::Value::Object(extra) = payload {
            obj.extend(extra);
        }
        self.events.push(serde_json::Value::Object(obj));
    }

    // Only the audio callback knows which buffer reached the file, so events carry the raw
    // clock until it has said so and are rebased onto the first captured frame here.
    fn rebase_frames(&mut self) {
        let origin = self
            .capture_start
            .load(std::sync::atomic::Ordering::Relaxed);
        for event in self.events.iter_mut() {
            let Some(frame) = event.get("frame").and_then(serde_json::Value::as_u64) else {
                continue;
            };
            let Some(object) = event.as_object_mut() else {
                continue;
            };
            if origin == audio::NOT_CAPTURING {
                object.remove("frame");
            } else {
                object.insert(
                    "frame".into(),
                    serde_json::json!(frame.saturating_sub(origin)),
                );
            }
        }
    }

    fn stop(&mut self) {
        self.rebase_frames();
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

/// Owns the session log for a recording: nothing outside here touches the `Option`,
/// so "is a session being recorded" is answered in one place.
pub(crate) struct Recorder {
    logger: Mutex<Option<SessionLogger>>,
}

impl Recorder {
    pub(crate) fn new() -> Self {
        Self {
            logger: Mutex::new(None),
        }
    }

    pub(crate) fn start(
        &self,
        mixer: &'static session_core::MixerManifest,
        capture_start: Arc<std::sync::atomic::AtomicU64>,
        anchor: RenderFrame,
        start_events: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
    ) {
        let mut logger = SessionLogger::new(mixer, capture_start);
        for (event_type, payload) in start_events {
            logger.log_at(anchor.get(), event_type, payload);
        }
        *self.logger.locked() = Some(logger);
    }

    pub(crate) fn stop(&self, frame: RenderFrame) {
        if let Some(logger) = self.logger.locked().as_mut() {
            logger.log_at(frame.get(), "recording_stop", serde_json::json!({}));
            logger.stop();
        }
    }

    pub(crate) fn take_pending(&self) -> Option<String> {
        self.logger
            .locked()
            .as_mut()
            .and_then(SessionLogger::take_pending)
    }

    pub(crate) fn log_at(&self, frame: RenderFrame, event_type: &str, payload: serde_json::Value) {
        if let Some(logger) = self.logger.locked().as_mut() {
            logger.log_at(frame.get(), event_type, payload);
        }
    }

    pub(crate) fn log_param_at(
        &self,
        frame: RenderFrame,
        deck: Option<&str>,
        slot: &str,
        param: &str,
        value: f32,
    ) {
        let mut payload = serde_json::Map::new();
        if let Some(deck) = deck {
            payload.insert("deck".into(), serde_json::json!(deck));
        }
        payload.insert("slot".into(), serde_json::json!(slot));
        payload.insert("param".into(), serde_json::json!(param));
        payload.insert("value".into(), f32_json(value));
        self.log_at(frame, "set_param", serde_json::Value::Object(payload));
    }
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
        assert_eq!(
            system_time_to_iso8601(unix_millis(1_000_123)),
            "1970-01-01T00:16:40.123Z"
        );
    }

    #[test]
    fn iso8601_leap_year_feb_29() {
        assert_eq!(
            system_time_to_iso8601(unix_secs(951_782_400)),
            "2000-02-29T00:00:00.000Z"
        );
    }

    #[test]
    fn iso8601_end_of_year() {
        assert_eq!(
            system_time_to_iso8601(unix_secs(946_684_799)),
            "1999-12-31T23:59:59.000Z"
        );
    }

    #[test]
    fn session_logger_records_events_in_order() {
        let mut logger = SessionLogger::new(
            &session_core::CLASSIC_3BAND,
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
        );
        logger.log_at(0, "first", serde_json::json!({}));
        logger.log_at(0, "second", serde_json::json!({}));
        logger.log_at(0, "third", serde_json::json!({}));
        assert_eq!(logger.events.len(), 3);
        assert_eq!(logger.events[0]["type"], "first");
        assert_eq!(logger.events[1]["type"], "second");
        assert_eq!(logger.events[2]["type"], "third");
    }

    #[test]
    fn a_logged_param_is_spelled_as_the_f32_the_mixer_acts_on() {
        assert_eq!(f64::from(0.95f32), 0.949999988079071);
        assert_eq!(f32_json(0.95f32).to_string(), "0.95");
    }

    #[test]
    fn narrowing_the_spelling_keeps_the_value() {
        for value in [0.95f32, 0.123456789, -0.0499999, 1.0, 0.0, -1.0, 6.0, -26.0] {
            let written = f32_json(value);
            let read_back = written.as_f64().map(|wide| wide as f32);
            assert_eq!(read_back, Some(value), "{written} lost {value}");
        }
    }

    #[test]
    fn a_logged_frame_is_the_one_captured_at_the_mutation() {
        let frames = Arc::new(std::sync::atomic::AtomicU64::new(4096));
        let mut logger = SessionLogger::new(
            &session_core::CLASSIC_3BAND,
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
        );

        let at_mutation = frames.load(std::sync::atomic::Ordering::Relaxed);
        // A callback completes before the event reaches the log.
        frames.store(4096 + 512, std::sync::atomic::Ordering::Relaxed);
        logger.log_at(at_mutation, "play", serde_json::json!({ "deck": "A" }));

        assert_eq!(logger.events[0]["frame"], 4096);
    }

    #[test]
    fn frames_are_rebased_onto_the_buffer_the_tap_actually_captured() {
        let capture_start = Arc::new(std::sync::atomic::AtomicU64::new(audio::NOT_CAPTURING));
        let mut logger =
            SessionLogger::new(&session_core::CLASSIC_3BAND, Arc::clone(&capture_start));

        logger.log_at(8192, "recording_start", serde_json::json!({}));
        logger.log_at(8192 + 256, "play", serde_json::json!({ "deck": "A" }));
        capture_start.store(8192, std::sync::atomic::Ordering::Relaxed);
        logger.rebase_frames();

        assert_eq!(logger.events[0]["frame"], 0);
        assert_eq!(logger.events[1]["frame"], 256);
    }

    #[test]
    fn a_session_whose_audio_never_reached_the_file_carries_no_frames() {
        let mut logger = SessionLogger::new(
            &session_core::CLASSIC_3BAND,
            Arc::new(std::sync::atomic::AtomicU64::new(audio::NOT_CAPTURING)),
        );
        logger.log_at(512, "play", serde_json::json!({ "deck": "A" }));
        logger.rebase_frames();

        assert!(logger.events[0].get("frame").is_none());
    }

    #[test]
    fn session_logger_merges_payload_fields() {
        let mut logger = SessionLogger::new(
            &session_core::CLASSIC_3BAND,
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
        );
        logger.log_at(0, "play", serde_json::json!({ "deck": "A", "rate": 1.0 }));
        let event = &logger.events[0];
        assert_eq!(event["type"], "play");
        assert_eq!(event["deck"], "A");
        assert_eq!(event["rate"], 1.0);
    }

    #[test]
    fn session_logger_timestamps_are_non_negative() {
        let mut logger = SessionLogger::new(
            &session_core::CLASSIC_3BAND,
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
        );
        logger.log_at(0, "e", serde_json::json!({}));
        let elapsed_ms = logger.events[0]["elapsed_ms"].as_f64().unwrap();
        assert!(elapsed_ms >= 0.0);
    }

    #[test]
    fn session_logger_stop_produces_valid_json() {
        let mut logger = SessionLogger::new(
            &session_core::CLASSIC_3BAND,
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
        );
        logger.log_at(0, "recording_start", serde_json::json!({}));
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

    #[test]
    fn session_logger_stamps_the_mixer_it_recorded_on() {
        let mut logger = SessionLogger::new(
            &session_core::CLASSIC_3BAND,
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
        );
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

    #[test]
    fn session_logger_stamps_the_mixer_it_was_given() {
        let mut logger = SessionLogger::new(
            &session_core::ISOLATOR_3BAND,
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
        );
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
        let mut logger = SessionLogger::new(
            &session_core::CLASSIC_3BAND,
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
        );
        logger.stop();
        assert!(logger.take_pending().is_some());
        assert!(logger.take_pending().is_none());
    }

    #[test]
    fn session_logger_stop_clears_events() {
        let mut logger = SessionLogger::new(
            &session_core::CLASSIC_3BAND,
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
        );
        logger.log_at(0, "a", serde_json::json!({}));
        logger.log_at(0, "b", serde_json::json!({}));
        logger.stop();
        assert!(logger.events.is_empty());
    }
}
