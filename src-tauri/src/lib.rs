mod audio;
mod broadcast;
mod commands;
mod deck_sync;
pub(crate) mod engine;
mod engine_push;
mod midi;
pub mod offline_render;
mod recorder;
pub(crate) mod session_playback;
pub mod settings;

use audio::AppAudio;
use engine_push::ParamOrigin;
use std::sync::Arc;

use tauri::menu::{
    AboutMetadataBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::Emitter;
use tauri::Manager;

/// Whether a control surface may drive the decks. Only performance mode allows it: in the
/// others the session scheduler writes the strips past the `set_deck_param` funnel.
pub struct SurfaceControl(std::sync::atomic::AtomicBool);

impl Default for SurfaceControl {
    /// The app starts in performance mode, and the frontend only pushes a mode on a change.
    fn default() -> Self {
        let control = Self(std::sync::atomic::AtomicBool::new(false));
        control.allow(AppMode::Performance);
        control
    }
}

impl SurfaceControl {
    pub(crate) fn allow(&self, mode: AppMode) {
        self.0.store(
            mode == AppMode::Performance,
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    pub(crate) fn allowed(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
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
    let engine = engine::Engine::new(Arc::new(audio));

    // Cloned before `.manage()` consumes app_state, so the broadcaster thread
    // can read every deck's live state (same reason ended_flags is cloned above).
    let audio_for_broadcast = Arc::clone(&engine.audio);
    let audio_for_push = Arc::clone(&engine.audio);
    let engine_push = Arc::clone(&engine.engine_push);

    tauri::Builder::default()
        .manage(engine)
        .manage(session_playback::SessionLibrary::new())
        .manage(SurfaceControl::default())
        .manage(midi::MidiState::new())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(move |app| {
            install_menu(app)?;
            install_logging(app, app_level)?;
            spawn_track_ended_poller(app, ended_flags);
            match app.path().app_data_dir() {
                Ok(data_dir) => broadcast::start(data_dir, audio_for_broadcast),
                Err(error) => log::warn!("performer broadcast disabled: {error}"),
            }
            engine_push::start(app.handle().clone(), audio_for_push, engine_push);
            wire_midi(app, &app.state::<midi::MidiState>());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::confirm_quit,
            commands::analyze_track,
            commands::discard_recording,
            commands::render_session_to_file,
            commands::save_bms_only,
            commands::eject_track,
            commands::files_info,
            commands::get_deck_levels,
            commands::get_master_level,
            commands::get_spectral_waveform_region,
            commands::get_track_amplitude_region,
            commands::list_audio_devices,
            commands::load_track,
            commands::list_midi_devices,
            commands::set_midi_device_deck,
            commands::set_midi_monitor,
            commands::pick_save_path,
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
            commands::set_cue_device,
            commands::set_cue_mix,
            commands::set_deck_param,
            commands::set_limiter_enabled,
            commands::set_loop_active,
            commands::set_loop_in,
            commands::set_loop_out,
            commands::set_main_device,
            commands::set_master_gain,
            commands::set_nudge,
            commands::set_playback_rate,
            commands::set_quantize,
            commands::set_reloop,
            commands::set_deck_muted,
            commands::start_recording,
            commands::stop_recording,
            commands::stop,
            commands::toggle_play,
            commands::open_session_dialog,
            commands::preload_session,
            commands::start_session_playback,
            commands::stop_session_playback,
            commands::unload_session,
            commands::update_session_events,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn install_menu(app: &tauri::App) -> tauri::Result<()> {
    let about = AboutMetadataBuilder::new()
        .name(Some("Beatmatcher"))
        .copyright(Some(
            "Copyright 2026 Matias Berrutti\ngithub.com/berrutti/beatmatcher",
        ))
        .icon(app.default_window_icon().cloned())
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
    app.set_menu(MenuBuilder::new(app).item(&app_menu).build()?)?;
    app.on_menu_event(move |app, event| {
        if event.id() == &quit_id {
            app.emit("quit-requested", ()).ok();
        }
    });
    Ok(())
}

fn install_logging(app: &tauri::App, level: log::LevelFilter) -> tauri::Result<()> {
    app.handle().plugin(
        tauri_plugin_log::Builder::default()
            .level(level)
            // Symphonia logs every frame it parses at info.
            .level_for("symphonia_format_riff", log::LevelFilter::Warn)
            .level_for("symphonia_format_isomp4", log::LevelFilter::Warn)
            .level_for("symphonia_metadata", log::LevelFilter::Warn)
            .level_for("symphonia_bundle_mp3", log::LevelFilter::Warn)
            .build(),
    )
}

/// The audio thread cannot emit, so it raises a flag and this drains them.
fn spawn_track_ended_poller(
    app: &tauri::App,
    flags: Vec<(String, Arc<std::sync::atomic::AtomicBool>)>,
) {
    let handle = app.handle().clone();
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
}

fn wire_midi(app: &tauri::App, midi_state: &midi::MidiState) {
    midi::start_monitor(app.handle().clone(), midi_state);

    // Both closures outlive this call, so they hold a handle and look state up when they
    // fire; a borrowed `State<'_, T>` cannot cross onto the MIDI thread.
    let dispatch_handle = app.handle().clone();
    midi::set_dispatch(
        midi_state,
        Arc::new(move |port: &str, data: &[u8]| {
            midi::apply(
                dispatch_handle.state::<engine::Engine>().inner(),
                dispatch_handle.state::<SurfaceControl>().inner(),
                dispatch_handle.state::<midi::MidiState>().inner(),
                &dispatch_handle,
                port,
                data,
            );
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_surface_drives_the_decks_before_any_mode_is_set() {
        assert!(SurfaceControl::default().allowed());
    }

    #[test]
    fn only_performance_mode_lets_a_surface_drive_the_decks() {
        let surface = SurfaceControl::default();
        for (mode, expected) in [
            (AppMode::Session, false),
            (AppMode::Edit, false),
            (AppMode::Performance, true),
        ] {
            surface.allow(mode);
            assert_eq!(surface.allowed(), expected, "{mode:?}");
        }
    }

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
}
