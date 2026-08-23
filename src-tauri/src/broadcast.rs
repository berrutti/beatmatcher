use crate::audio::AppAudio;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(unix)]
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// Live decks only. The edit deck E is never broadcast.

const BROADCAST_INTERVAL_MS: u64 = 50;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DeckBroadcast {
    id: String,
    is_playing: bool,
    bpm: Option<f64>,
    beat_offset_sec: f64,
    position_sec: f64,
    playback_rate: f64,
    jog_hold_factor: f64,
    effective_bpm: Option<f64>,
    current_beat: Option<f64>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StateBroadcast {
    schema_version: u32,
    epoch_ms: u128,
    sample_rate: u32,
    decks: Vec<DeckBroadcast>,
}

fn deck_broadcast(audio: &AppAudio, id: &str) -> DeckBroadcast {
    let Some(deck_arc) = audio.deck(id) else {
        return DeckBroadcast {
            id: id.to_string(),
            is_playing: false,
            bpm: None,
            beat_offset_sec: 0.0,
            position_sec: 0.0,
            playback_rate: 1.0,
            jog_hold_factor: 1.0,
            effective_bpm: None,
            current_beat: None,
        };
    };
    let deck = deck_arc.lock().unwrap_or_else(|error| error.into_inner());
    let sample_rate = deck.device_sample_rate as f64;
    let position_sec = if sample_rate > 0.0 {
        deck.main_pos / sample_rate
    } else {
        0.0
    };
    let beat_offset_sec = if sample_rate > 0.0 {
        deck.beat_offset_frames / sample_rate
    } else {
        0.0
    };
    DeckBroadcast {
        id: id.to_string(),
        is_playing: deck.is_playing,
        bpm: deck.bpm,
        beat_offset_sec,
        position_sec,
        playback_rate: deck.playback_rate,
        jog_hold_factor: deck.jog_hold_factor,
        effective_bpm: deck
            .bpm
            .map(|bpm| bpm * deck.playback_rate * deck.jog_hold_factor),
        current_beat: deck
            .bpm
            .map(|bpm| session_core::current_beat(position_sec, beat_offset_sec, bpm)),
    }
}

fn snapshot(audio: &AppAudio) -> StateBroadcast {
    let epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    StateBroadcast {
        schema_version: 1,
        epoch_ms,
        sample_rate: audio.device_sample_rate,
        decks: crate::audio::LIVE_DECK_IDS
            .iter()
            .map(|&id| deck_broadcast(audio, id))
            .collect(),
    }
}

struct Sinks {
    file_path: PathBuf,
    tmp_path: PathBuf,
    #[cfg(unix)]
    clients: Arc<Mutex<Vec<std::os::unix::net::UnixStream>>>,
}

impl Sinks {
    fn new(data_dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&data_dir)?;
        #[cfg(unix)]
        let clients = spawn_socket_listener(&data_dir)?;
        Ok(Self {
            file_path: data_dir.join("state.json"),
            tmp_path: data_dir.join("state.json.tmp"),
            #[cfg(unix)]
            clients,
        })
    }

    fn publish(&self, json: &str) {
        // Atomic file replace: write to a temp file then rename onto state.json
        // so a reader never observes a half-written file (rename is atomic on the
        // same filesystem).
        if std::fs::write(&self.tmp_path, json).is_ok() {
            let _ = std::fs::rename(&self.tmp_path, &self.file_path);
        }
        #[cfg(unix)]
        self.publish_socket(json);
    }

    #[cfg(unix)]
    fn publish_socket(&self, json: &str) {
        use std::io::{ErrorKind, Write};
        let frame = format!("{json}\n");
        let mut clients = self
            .clients
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        clients.retain_mut(|stream| match stream.write_all(frame.as_bytes()) {
            Ok(()) => true,
            // A slow reader's buffer is full: keep the client, drop this frame only.
            Err(error) if error.kind() == ErrorKind::WouldBlock => true,
            Err(_) => false,
        });
    }
}

#[cfg(unix)]
fn spawn_socket_listener(
    data_dir: &std::path::Path,
) -> std::io::Result<Arc<Mutex<Vec<std::os::unix::net::UnixStream>>>> {
    let socket_path = data_dir.join("beatmatcher.sock");
    // A stale socket file from a previous run would make bind fail with EADDRINUSE.
    let _ = std::fs::remove_file(&socket_path);
    let listener = std::os::unix::net::UnixListener::bind(&socket_path)?;
    let clients: Arc<Mutex<Vec<std::os::unix::net::UnixStream>>> = Arc::new(Mutex::new(Vec::new()));
    let accepted = clients.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let _ = stream.set_nonblocking(true);
                    accepted
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .push(stream);
                }
                Err(_) => break,
            }
        }
    });
    Ok(clients)
}

// Publishes a per-deck state snapshot to state.json (all platforms) and a Unix
// domain socket (unix only) on a fixed interval, for an external app (the
// performer) to phase-sync to. Best-effort: a failure to set up the sinks
// disables broadcasting rather than aborting startup.
pub fn start(data_dir: PathBuf, audio: Arc<AppAudio>) {
    let sinks = match Sinks::new(data_dir) {
        Ok(sinks) => sinks,
        Err(error) => {
            log::warn!("performer broadcast disabled: {error}");
            return;
        }
    };
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(BROADCAST_INTERVAL_MS));
        if let Ok(json) = serde_json::to_string(&snapshot(&audio)) {
            sinks.publish(&json);
        }
    });
}
