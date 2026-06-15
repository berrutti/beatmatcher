// Shared per-deck application of a `SessionCommand` against the real
// `DeckState`/`ChannelStrip`, used by both the live scheduler
// (session_playback::apply_event_live) and the offline renderer
// (offline_render::apply_event). Loading is the one part that differs between
// the two (pre-decoded cache vs decode-on-demand), so it's injected via
// `load_samples`. `overshoot_frames` drives `compensate_late_start` for live
// playback; the offline renderer passes 0.0, which is a no-op.

use super::{ChannelStrip, DeckState};
use session_core::event::SessionCommand;
use std::sync::Arc;

// (samples, channels) for a decoded track, returned by the caller-provided loader.
type LoadedSamples = (Arc<Vec<f32>>, usize);
type LoadSamples<'a> = dyn FnMut(&str) -> Result<LoadedSamples, String> + 'a;

pub(crate) fn apply_deck_command(
    cmd: &SessionCommand<'_>,
    deck: &mut DeckState,
    strip: &mut ChannelStrip,
    sr: u32,
    overshoot_frames: f64,
    load_samples: &mut LoadSamples<'_>,
) -> Result<(), String> {
    let sr_f = sr as f64;
    let overshoot_f = overshoot_frames;

    macro_rules! to_frames {
        ($sec:expr) => {
            ($sec * sr_f).clamp(0.0, deck.total_frames as f64)
        };
    }

    match *cmd {
        SessionCommand::DeckSnapshot {
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
            load_into(deck, path, sr, load_samples)?;
            if let Some(pos) = position_sec {
                let f = to_frames!(pos);
                deck.main_pos = f;
                deck.cue_pos = f;
            }
            if let Some(cp) = cue_point_sec {
                deck.cue_point = to_frames!(cp);
            }
            if let Some(bpm) = bpm {
                deck.bpm = Some(bpm);
            }
            if let Some(rate) = playback_rate {
                deck.playback_rate = rate.max(0.1);
            }
            if let Some(la) = loop_active {
                deck.loop_active = la;
            }
            if let Some(le) = loop_end_sec {
                deck.loop_end = to_frames!(le);
            }
            if is_playing {
                deck.is_playing = true;
            }
        }

        SessionCommand::LoadTrack {
            path,
            beat_offset_sec,
            ..
        } => {
            load_into(deck, path, sr, load_samples)?;
            if let Some(offset) = beat_offset_sec {
                let f = to_frames!(offset);
                deck.main_pos = f;
                deck.cue_pos = f;
                deck.cue_point = f;
            }
        }

        SessionCommand::EjectTrack { .. } => {
            deck.eject();
        }

        SessionCommand::Play { sec, .. } => {
            let was_playing = deck.is_playing;
            if let Some(sec) = sec {
                let f = to_frames!(sec);
                deck.main_pos = f;
                deck.cue_pos = f;
            } else {
                deck.cue_pos = deck.main_pos;
            }
            deck.is_playing = true;
            // Only align a deck that actually (re)starts or is repositioned here.
            // A bare `play` latch on a deck that is ALREADY playing (e.g. pressing
            // PLAY to latch out of a held cue preview) must not move the playhead:
            // compensating it forward skips ~one buffer of audio mid-playback,
            // heard as a single click.
            if sec.is_some() || !was_playing {
                deck.compensate_late_start(overshoot_f);
            }
        }

        SessionCommand::Stop { .. } => {
            deck.is_playing = false;
        }

        SessionCommand::StopAtCue { cue_point_sec, .. } => {
            deck.is_playing = false;
            if let Some(cp) = cue_point_sec {
                let f = to_frames!(cp);
                deck.main_pos = f;
                deck.cue_pos = f;
            }
        }

        SessionCommand::Seek { sec, .. } => {
            let f = to_frames!(sec);
            deck.main_pos = f;
            deck.cue_pos = f;
            if deck.loop_active && (f < deck.cue_point || f >= deck.loop_end) {
                deck.loop_active = false;
            }
            deck.compensate_late_start(overshoot_f);
        }

        SessionCommand::SetVolume { gain, .. } => {
            strip.set_gain(gain);
        }

        SessionCommand::SetEq { band, db, .. } => {
            strip.set_eq_band(band, db);
        }

        SessionCommand::SetFilter { value, .. } => {
            strip.set_filter(value);
        }

        SessionCommand::SetFilterActive { active, .. } => {
            strip.set_filter_active(active);
        }

        SessionCommand::SetPlaybackRate { rate, .. } => {
            deck.playback_rate = rate.max(0.1);
        }

        SessionCommand::SetNudge { percent, .. } => {
            deck.nudge_factor = 1.0 + percent / 100.0;
        }

        SessionCommand::SetMasterGain { .. } => {
            unreachable!("SetMasterGain has no deck; callers dispatch it separately")
        }

        SessionCommand::SetBeatGrid {
            bpm,
            beat_offset_sec,
            ..
        } => {
            if let Some(bpm) = bpm {
                deck.bpm = Some(bpm);
            }
            if let Some(off) = beat_offset_sec {
                deck.beat_offset_frames = off * sr_f;
            }
        }

        SessionCommand::LoopIn { cue_sec, .. } => {
            if let Some(cs) = cue_sec {
                deck.cue_point = to_frames!(cs);
            }
            deck.loop_active = false;
            deck.loop_end = 0.0;
        }

        SessionCommand::LoopOut {
            start_sec, end_sec, ..
        } => {
            if let Some(ss) = start_sec {
                deck.cue_point = to_frames!(ss);
            }
            if let Some(es) = end_sec {
                deck.loop_end = to_frames!(es);
            }
            deck.loop_active = true;
        }

        SessionCommand::ExitLoop { .. } => {
            deck.loop_active = false;
        }

        SessionCommand::Reloop { .. } => {
            if deck.loop_end > deck.cue_point {
                deck.main_pos = deck.cue_point;
                deck.cue_pos = deck.cue_point;
                if deck.is_playing {
                    deck.loop_active = true;
                }
                deck.compensate_late_start(overshoot_f);
            }
        }

        // Holding CUE plays from the cue point through the main path. Audible in the recording.
        SessionCommand::CuePreviewStart { cue_point_sec, .. } => {
            let cp = cue_point_sec.unwrap_or(deck.cue_point / sr_f);
            let f = to_frames!(cp);
            deck.cue_point = f;
            deck.main_pos = f;
            deck.cue_pos = f;
            deck.is_playing = true;
            deck.is_cueing = true;
            deck.compensate_late_start(overshoot_f);
        }

        // Releasing CUE stops playback and returns to cue point.
        SessionCommand::CuePreviewEnd { cue_point_sec, .. } => {
            let cp = cue_point_sec.unwrap_or(deck.cue_point / sr_f);
            let f = to_frames!(cp);
            deck.is_playing = false;
            deck.is_cueing = false;
            deck.main_pos = f;
            deck.cue_pos = f;
        }
    }

    Ok(())
}

// Shared load body for DeckSnapshot and LoadTrack: full reset, then the
// samples from `load_samples` installed.
fn load_into(
    deck: &mut DeckState,
    path: &str,
    sr: u32,
    load_samples: &mut LoadSamples<'_>,
) -> Result<(), String> {
    let (samples, channels) = load_samples(path)?;
    let total_frames = samples.len() / channels;
    deck.reset();
    deck.samples = samples;
    deck.channels = channels;
    deck.device_sample_rate = sr;
    deck.total_frames = total_frames;
    deck.duration = total_frames as f64 / sr as f64;
    deck.loaded_path = Some(path.to_string());
    Ok(())
}
