mod analysis;
mod atomic_f32;
mod deck;
mod dsp;
mod io;
mod recording;
mod session_apply;
mod stream;
mod unit;

pub use analysis::{
    compute_amplitude_region, compute_spectral_waveform_region, detect_bpm, detect_silence_end,
    reduce_packets, BandStream, StreamedBands, DENSE_POINTS_PER_SEC,
};
pub(crate) use deck::DeckRestore;
#[cfg(test)]
pub(crate) use deck::RenderTargets;
pub use deck::{logged_jog_ticks, ChannelStrip, CuePressOutcome, Deck, RenderFrame, SpectralBands};
pub(crate) use dsp::Limiter;
pub(crate) use dsp::{master_block, master_output, FILTER_CROSSFADE_TAU_SEC};
pub use io::TrackTags;
pub use io::{
    decode_audio, decode_audio_streaming, read_cover_art, read_tags, resample_linear,
    resample_packets, DecodedPacket,
};
pub(crate) use session_apply::{apply_deck_command, LoadedSamples};
pub use stream::MasterMonitor;
pub(crate) use stream::DEFAULT_MASTER_GAIN;
pub(crate) use stream::NOT_CAPTURING;
pub(crate) use stream::{channel_pairs, mix_channels, ChannelPairs};
#[cfg(test)]
pub(crate) use unit::approach;

use crate::lock::LockIgnoringPoison;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc, Mutex,
};

use analysis::{BPM_MAX, BPM_MIN};
use recording::{flac_writer_thread, wav_writer_thread, Recording};
pub(crate) use recording::{save_permille, SAVE_DONE, SAVE_IDLE};
use stream::{
    best_output_config, build_combined_stream, build_cue_stream, build_stream, find_output_device,
    MasterMonitor as Monitor, SendStream,
};

/// Stands in only until the frontend mirrors its own setting down, which it does
/// on load. Matches the default in `settings.ts`.
const DEFAULT_PITCH_RANGE_PERCENT: f64 = 10.0;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfo {
    pub duration: f64,
    pub sample_rate: u32,
    pub bpm: Option<f64>,
    pub silence_end: f64,
    pub cover_art: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub channels: usize,
}

// The one mixer the live engine builds. The offline renderer builds whichever a `.bms`
// names (see `session_core::MANIFESTS`), but nothing selects another at runtime.
pub(crate) const MIXER: &session_core::MixerManifest = &session_core::CLASSIC_3BAND_V2;

pub(crate) use dsp::FILTER_COEFF_REFRESH_INTERVAL;

/// Every deck that reaches the live mixer. The edit deck is deliberately absent.
pub(crate) const LIVE_DECK_IDS: [&str; 4] = ["A", "B", "C", "D"];

/// Session view only, so it never reaches the mixer, a recording or a broadcast.
pub(crate) const EDIT_DECK_ID: &str = "E";

#[derive(Clone)]
struct Output {
    device_id: String,
    channel_offset: usize,
}

/// `cue` is `None` when no headphone device is configured.
#[derive(Clone)]
struct Routing {
    main: Output,
    cue: Option<Output>,
}

pub struct AppAudio {
    pub device_sample_rate: u32,
    decks: HashMap<String, Arc<Mutex<Deck>>>,
    strips: HashMap<String, Arc<Mutex<ChannelStrip>>>,
    default_device_id: String,
    routing: Mutex<Routing>,
    buffer_frames: Arc<AtomicU32>,
    pub bpm_min: Arc<AtomicU32>,
    pub bpm_max: Arc<AtomicU32>,
    // Mirrored from the frontend, which owns it. A tempo fader arrives as a
    // position that means nothing until something knows how far the fader travels.
    pitch_range_percent: Mutex<f64>,
    main_stream: Mutex<Option<SendStream>>,
    cue_stream: Mutex<Option<SendStream>>,
    pub monitor: MasterMonitor,
    recording: Mutex<Option<Recording>>,
    // Permille of the save that runs after the last frame is captured. Only the
    // FLAC encode has one: a WAV is already on disk by then.
    save_progress: Arc<AtomicU32>,
    mixer: &'static session_core::MixerManifest,
}

/// Clears the save dial on drop, so an early return cannot strand it mid-permille.
pub(crate) struct SaveProgress(Arc<AtomicU32>);

impl SaveProgress {
    /// A copy for whatever thread does the work; the guard keeps its own.
    pub(crate) fn share(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.0)
    }
}

impl Drop for SaveProgress {
    fn drop(&mut self) {
        self.0.store(SAVE_IDLE, Ordering::Relaxed);
    }
}

impl AppAudio {
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no default output device")?;
        let default_device_id = device.name().unwrap_or_default();
        let config = device.default_output_config()?;
        let device_sample_rate = config.sample_rate().0;

        let audio = Self::unrouted(device_sample_rate, default_device_id);
        let channels = channel_pairs(&audio.decks, &audio.strips);
        let main_stream = build_stream(
            &device,
            &config,
            channels,
            false,
            0,
            Some(audio.monitor.clone()),
            0,
        )?;
        main_stream.play()?;
        *audio.main_stream.locked() = Some(SendStream(main_stream));
        Ok(audio)
    }

    /// Everything except the output stream, which is the only part that needs a real
    /// device, so a test can drive the engine without one.
    pub(crate) fn unrouted(device_sample_rate: u32, default_device_id: String) -> Self {
        let mut decks = HashMap::new();
        let mut strips = HashMap::new();
        for id in LIVE_DECK_IDS.into_iter().chain([EDIT_DECK_ID]) {
            let deck = Deck::empty(device_sample_rate);
            decks.insert(id.to_string(), Arc::new(Mutex::new(deck)));
            strips.insert(
                id.to_string(),
                Arc::new(Mutex::new(ChannelStrip::from_manifest(
                    MIXER,
                    device_sample_rate as f32,
                ))),
            );
        }

        Self {
            device_sample_rate,
            decks,
            strips,
            routing: Mutex::new(Routing {
                main: Output {
                    device_id: default_device_id.clone(),
                    channel_offset: 0,
                },
                cue: None,
            }),
            buffer_frames: Arc::new(AtomicU32::new(0)),
            bpm_min: Arc::new(AtomicU32::new(BPM_MIN as u32)),
            bpm_max: Arc::new(AtomicU32::new(BPM_MAX as u32)),
            pitch_range_percent: Mutex::new(DEFAULT_PITCH_RANGE_PERCENT),
            default_device_id,
            main_stream: Mutex::new(None),
            cue_stream: Mutex::new(None),
            monitor: Monitor::new(),
            recording: Mutex::new(None),
            save_progress: Arc::new(AtomicU32::new(SAVE_IDLE)),
            mixer: MIXER,
        }
    }

    pub fn deck(&self, id: &str) -> Option<Arc<Mutex<Deck>>> {
        self.decks.get(id).cloned()
    }

    /// The flag each deck's audio thread raises at the end of its track, shared with the
    /// deck itself rather than held twice.
    pub fn ended_flags(&self) -> Vec<(String, Arc<AtomicBool>)> {
        self.decks
            .iter()
            .map(|(id, deck)| (id.clone(), Arc::clone(&deck.locked().just_ended)))
            .collect()
    }

    pub fn strip(&self, id: &str) -> Option<Arc<Mutex<ChannelStrip>>> {
        self.strips.get(id).cloned()
    }

    pub fn deck_ids(&self) -> Vec<String> {
        self.strips.keys().cloned().collect()
    }

    pub fn list_devices(&self) -> Vec<DeviceInfo> {
        let host = cpal::default_host();
        // `output_devices()` filters on max_output_channels > 0, which drops a Bluetooth
        // or USB device that is not the current system output on macOS.
        host.devices()
            .map(|devices| {
                devices
                    .filter_map(|d| {
                        let name = d.name().ok()?;
                        let mut configs = d.supported_output_configs().ok()?.peekable();
                        configs.peek()?;
                        let max_channels =
                            configs.map(|c| c.channels() as usize).max().unwrap_or(2);
                        let is_default = name == self.default_device_id;
                        Some(DeviceInfo {
                            id: name.clone(),
                            name,
                            is_default,
                            channels: max_channels,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn set_cue_device(&self, device_id: &str, channel_offset: usize) -> Result<(), String> {
        log::info!(
            "set_cue_device: id='{}' channel_offset={}",
            device_id,
            channel_offset
        );
        self.routing.locked().cue = (!device_id.is_empty()).then(|| Output {
            device_id: device_id.to_string(),
            channel_offset,
        });
        self.rebuild_streams()
    }

    pub fn set_main_device(&self, device_id: &str, channel_offset: usize) -> Result<(), String> {
        log::info!(
            "set_main_device: id='{}' channel_offset={}",
            device_id,
            channel_offset
        );
        self.routing.locked().main = Output {
            device_id: device_id.to_string(),
            channel_offset,
        };
        self.rebuild_streams()
    }

    /// What every strip is built on, so a recording is stamped with the mixer it played
    /// through and a `.bms` naming another can be refused over replayed at wrong scales.
    pub fn mixer(&self) -> &'static session_core::MixerManifest {
        self.mixer
    }

    pub fn set_bpm_range(&self, min: u32, max: u32) {
        self.bpm_min.store(min, Ordering::Relaxed);
        self.bpm_max.store(max, Ordering::Relaxed);
    }

    pub fn pitch_range_percent(&self) -> f64 {
        *self.pitch_range_percent.locked()
    }

    pub fn set_jog_rotation_speed(&self, speed: session_core::JogRotationSpeed) {
        for deck in self.decks.values() {
            deck.locked().set_jog_rotation_speed(speed);
        }
    }

    pub fn release_held_controls(&self) {
        for deck in self.decks.values() {
            deck.locked().release_held_controls();
        }
    }

    /// Every deck is set together, so deck A answers for all of them.
    pub fn jog_rotation_speed(&self) -> session_core::JogRotationSpeed {
        self.decks
            .get("A")
            .map(|deck| deck.locked().jog_rotation_speed)
            .unwrap_or_default()
    }

    pub fn set_pitch_range_percent(&self, percent: f64) {
        *self.pitch_range_percent.locked() = percent;
    }

    pub fn set_buffer_frames(&self, frames: u32) -> Result<(), String> {
        log::info!("set_buffer_frames: {}", frames);
        self.buffer_frames.store(frames, Ordering::Relaxed);
        self.rebuild_streams()
    }

    // One callback when main and cue share a device: two CoreAudio callbacks interfere,
    // and the cue one writing zeros blanks the main output.
    fn rebuild_streams(&self) -> Result<(), String> {
        self.monitor.restart_period();
        let routing = self.routing.locked().clone();
        let main_id = routing.main.device_id;
        let main_off = routing.main.channel_offset;
        let cue = routing.cue;
        let cue_id = cue
            .as_ref()
            .map_or("", |o| o.device_id.as_str())
            .to_string();
        let cue_off = cue.as_ref().map_or(0, |o| o.channel_offset);
        let buf_frames = self.buffer_frames.load(Ordering::Relaxed);

        log::info!(
            "rebuild_streams: main='{}' off={} | cue='{}' off={} buf={}",
            main_id,
            main_off,
            cue_id,
            cue_off,
            buf_frames
        );

        let ch = channel_pairs(&self.decks, &self.strips);
        let monitor = self.monitor.clone();

        if !cue_id.is_empty() && cue_id == main_id {
            // Same device. One combined stream handles both master (ch main_off/main_off+1)
            // and cue (ch cue_off/cue_off+1) in a single callback.
            let device = find_output_device(&main_id)?;
            let min_ch = (main_off + 2).max(cue_off + 2);
            let config = best_output_config(&device, min_ch, self.device_sample_rate)?;
            log::info!(
                "rebuild_streams: combined config ch={} sr={} fmt={:?}",
                config.channels(),
                config.sample_rate().0,
                config.sample_format()
            );
            let stream = build_combined_stream(
                &device,
                &config,
                stream::CombinedStreamParams {
                    channels: ch,
                    output_channels: config.channels() as usize,
                    main_offset: main_off,
                    cue_offset: cue_off,
                    monitor,
                    buffer_frames: buf_frames,
                },
            )
            .map_err(|e| e.to_string())?;

            if let Some(s) = self.main_stream.locked().as_ref() {
                s.0.pause().ok();
            }
            {
                let mut guard = self.cue_stream.locked();
                if let Some(s) = guard.as_ref() {
                    s.0.pause().ok();
                }
                *guard = None;
            }
            self.sync_cue_positions();
            {
                let mut guard = self.main_stream.locked();
                *guard = Some(SendStream(stream));
                guard
                    .as_ref()
                    .unwrap()
                    .0
                    .play()
                    .map_err(|e| e.to_string())?;
            }
            log::info!("rebuild_streams: combined stream playing");
        } else {
            // Different devices, or main is unset, or no cue configured.
            // Build all new streams before pausing anything so the gap is minimal.
            let new_main_stream = if !main_id.is_empty() {
                let main_device = find_output_device(&main_id)?;
                let main_cfg =
                    best_output_config(&main_device, main_off + 2, self.device_sample_rate)?;
                log::info!(
                    "rebuild_streams: master config ch={} sr={} fmt={:?}",
                    main_cfg.channels(),
                    main_cfg.sample_rate().0,
                    main_cfg.sample_format()
                );
                Some(
                    build_stream(
                        &main_device,
                        &main_cfg,
                        ch.clone(),
                        false,
                        main_off,
                        Some(monitor.clone()),
                        buf_frames,
                    )
                    .map_err(|e| e.to_string())?,
                )
            } else {
                log::info!("rebuild_streams: no master output configured");
                None
            };

            let new_cue_stream = if !cue_id.is_empty() {
                let cue_device = find_output_device(&cue_id)?;
                let cue_cfg =
                    best_output_config(&cue_device, cue_off + 2, self.device_sample_rate)?;
                log::info!(
                    "rebuild_streams: cue config ch={} sr={} fmt={:?}",
                    cue_cfg.channels(),
                    cue_cfg.sample_rate().0,
                    cue_cfg.sample_format()
                );
                // When there is no main output, the cue stream also drives the
                // master mix render so that recording and metering still work.
                let cue_monitor = if main_id.is_empty() {
                    Some(monitor)
                } else {
                    None
                };
                Some(
                    build_cue_stream(&cue_device, &cue_cfg, ch, cue_off, cue_monitor, buf_frames)
                        .map_err(|e| e.to_string())?,
                )
            } else {
                None
            };

            // Pause all old streams, sync cue_pos to main_pos, then start new streams.
            if let Some(s) = self.main_stream.locked().as_ref() {
                s.0.pause().ok();
            }
            {
                let guard = self.cue_stream.locked();
                if let Some(s) = guard.as_ref() {
                    s.0.pause().ok();
                }
            }
            self.sync_cue_positions();

            {
                let mut guard = self.main_stream.locked();
                match new_main_stream {
                    Some(s) => {
                        *guard = Some(SendStream(s));
                        guard
                            .as_ref()
                            .unwrap()
                            .0
                            .play()
                            .map_err(|e| e.to_string())?;
                    }
                    None => *guard = None,
                }
            }
            {
                let mut guard = self.cue_stream.locked();
                match new_cue_stream {
                    Some(s) => {
                        *guard = Some(SendStream(s));
                        guard
                            .as_ref()
                            .unwrap()
                            .0
                            .play()
                            .map_err(|e| e.to_string())?;
                    }
                    None => *guard = None,
                }
            }
            log::info!("rebuild_streams: separate streams playing");
        }

        Ok(())
    }

    fn sync_cue_positions(&self) {
        for deck_arc in self.decks.values() {
            let mut deck = deck_arc.locked();
            deck.cue_pos = deck.main_pos;
        }
    }

    pub fn get_master_level(&self) -> [f32; 2] {
        self.monitor.get_levels()
    }

    pub fn get_deck_levels(&self) -> HashMap<String, [f32; 2]> {
        self.strips
            .iter()
            .map(|(id, strip)| (id.clone(), strip.locked().get_level()))
            .collect()
    }

    /// Both the first frame the recording may contain and the stamp on its start events, so
    /// the capture and the snapshot describe the same instant.
    fn capture_anchor(&self) -> u64 {
        self.strips
            .values()
            .map(|strip| strip.locked().next_render_frame)
            .max()
            .unwrap_or_else(|| self.monitor.output_frames())
    }

    pub fn start_recording(
        &self,
        bit_depth: u16,
        use_flac: bool,
        temp_path: String,
    ) -> Result<u64, String> {
        let mut recording = self.recording.locked();
        if recording.is_some() {
            return Err("already recording".to_string());
        }

        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(256);
        let anchor;
        {
            let mut slot = self.monitor.record_tx.locked();
            anchor = self.capture_anchor();
            self.monitor.arm_capture(anchor);
            *slot = Some(tx);
        }
        let sr = self.device_sample_rate;
        let path_for_thread = temp_path.clone();
        let thread = if use_flac {
            let progress = Arc::clone(&self.save_progress);
            std::thread::spawn(move || {
                flac_writer_thread(path_for_thread, sr, bit_depth, rx, progress)
            })
        } else {
            // Streamed straight to disk, so stopping only back-patches the header.
            std::thread::spawn(move || wav_writer_thread(path_for_thread, sr, bit_depth, rx))
        };
        *recording = Some(Recording { thread, temp_path });
        Ok(anchor)
    }

    /// None when nothing is saving.
    pub fn save_progress(&self) -> Option<f64> {
        let permille = self.save_progress.load(Ordering::Relaxed);
        if permille == SAVE_IDLE {
            return None;
        }
        Some(f64::from(permille) / f64::from(SAVE_DONE))
    }

    /// Arms the dial and hands out the copy the work reports on. The returned guard
    /// clears it however the work ends, so no caller has to remember to.
    pub(crate) fn begin_save(&self) -> SaveProgress {
        self.save_progress.store(0, Ordering::Relaxed);
        SaveProgress(Arc::clone(&self.save_progress))
    }

    pub fn stop_recording(&self) -> Result<String, String> {
        self.monitor.record_tx.locked().take();
        let state = self.recording.locked().take();
        if let Some(s) = state {
            s.thread
                .join()
                .map_err(|_| "recorder thread panicked".to_string())??;
            Ok(s.temp_path)
        } else {
            Err("not recording".to_string())
        }
    }
}
