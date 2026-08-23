use super::{ChannelStrip, Deck};
use session_core::event::SessionCommand;
use std::sync::Arc;

// (samples, channels) for a decoded track, returned by the caller-provided loader.
pub(crate) type LoadedSamples = (Arc<Vec<f32>>, usize);
type LoadSamples<'a> = dyn FnMut(&str) -> Result<LoadedSamples, String> + 'a;

pub(crate) fn apply_deck_command(
    cmd: &SessionCommand<'_>,
    deck: &mut Deck,
    strip: &mut ChannelStrip,
    sample_rate: u32,
    overshoot_frames: f64,
    load_samples: &mut LoadSamples<'_>,
) -> Result<(), String> {
    let sample_rate_f64 = sample_rate as f64;
    let overshoot_f = overshoot_frames;

    macro_rules! to_frames {
        ($sec:expr) => {
            ($sec * sample_rate_f64).clamp(0.0, deck.total_frames as f64)
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
            load_into(deck, path, sample_rate, load_samples)?;
            if let Some(pos) = position_sec {
                let frames = to_frames!(pos);
                deck.main_pos = frames;
                deck.cue_pos = frames;
            }
            if let Some(cue_point_sec) = cue_point_sec {
                deck.cue_point = to_frames!(cue_point_sec);
            }
            if let Some(bpm) = bpm {
                deck.bpm = Some(bpm);
            }
            if let Some(rate) = playback_rate {
                deck.playback_rate = rate.max(0.1);
            }
            if let Some(loop_active) = loop_active {
                deck.loop_active = loop_active;
            }
            if let Some(loop_end_sec) = loop_end_sec {
                deck.loop_end = to_frames!(loop_end_sec);
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
            load_into(deck, path, sample_rate, load_samples)?;
            if let Some(offset) = beat_offset_sec {
                let frames = to_frames!(offset);
                deck.main_pos = frames;
                deck.cue_pos = frames;
                deck.cue_point = frames;
            }
        }

        SessionCommand::EjectTrack { .. } => {
            deck.eject();
        }

        SessionCommand::Play { sec, .. } => {
            let was_playing = deck.is_playing;
            if let Some(sec) = sec {
                let frames = to_frames!(sec);
                deck.main_pos = frames;
                deck.cue_pos = frames;
            } else {
                deck.cue_pos = deck.main_pos;
            }
            deck.is_playing = true;
            // Pressing play during a cue preview latches: the preview ends and the deck keeps
            // playing from where it reached, rather than snapping back on the next release.
            deck.is_cueing = false;
            // A bare `play` latch on an already-playing deck must not be compensated:
            // skipping a buffer mid-playback is heard as a single click.
            if sec.is_some() || !was_playing {
                deck.compensate_late_start(overshoot_f);
            }
        }

        SessionCommand::Stop { .. } => {
            deck.is_playing = false;
        }

        SessionCommand::StopAtCue { cue_point_sec, .. } => {
            deck.is_playing = false;
            if let Some(cue_point_sec) = cue_point_sec {
                let frames = to_frames!(cue_point_sec);
                deck.main_pos = frames;
                deck.cue_pos = frames;
            }
        }

        SessionCommand::Seek { sec, .. } => {
            let frames = to_frames!(sec);
            deck.main_pos = frames;
            deck.cue_pos = frames;
            if deck.outside_loop(frames) {
                deck.loop_active = false;
            }
            deck.compensate_late_start(overshoot_f);
        }

        SessionCommand::SetXfaderAssign { assign, .. } => {
            strip.set_xfader_assign(assign);
        }
        SessionCommand::SetFaderCurve { .. } => {}
        SessionCommand::SetJogRotationSpeed { .. } => {}
        SessionCommand::Jog { ticks, .. } => {
            deck.deposit_jog(ticks);
        }
        SessionCommand::SetParam {
            slot, param, value, ..
        } => match (slot, param) {
            ("fader", "gain") => strip.set_gain(value as f32),
            ("eq", band) => strip.set_eq_band(band, value as f32),
            ("filter", "value") => strip.set_filter(value as f32),
            ("filter", "active") => strip.set_filter_active(value != 0.0),
            _ => {}
        },

        SessionCommand::SetPlaybackRate { rate, .. } => {
            deck.playback_rate = rate.max(0.1);
        }

        SessionCommand::SetNudge { percent, .. } => {
            deck.set_nudge_percent(percent);
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
                deck.beat_offset_frames = off * sample_rate_f64;
            }
        }

        SessionCommand::LoopIn { cue_sec, .. } => {
            if let Some(cue_sec) = cue_sec {
                deck.cue_point = to_frames!(cue_sec);
            }
            deck.loop_active = false;
            deck.loop_end = 0.0;
        }

        SessionCommand::LoopOut {
            start_sec, end_sec, ..
        } => {
            if let Some(start_sec) = start_sec {
                deck.cue_point = to_frames!(start_sec);
            }
            if let Some(end_sec) = end_sec {
                deck.loop_end = to_frames!(end_sec);
            }
            deck.loop_active = true;
            let length = deck.loop_end - deck.cue_point;
            if length > 0.0 && deck.main_pos > deck.loop_end {
                deck.main_pos = deck.cue_point + (deck.main_pos - deck.loop_end) % length;
            }
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
            let cue_point_sec = cue_point_sec.unwrap_or(deck.cue_point / sample_rate_f64);
            let frames = to_frames!(cue_point_sec);
            deck.cue_point = frames;
            deck.main_pos = frames;
            deck.cue_pos = frames;
            deck.is_playing = true;
            deck.is_cueing = true;
            deck.compensate_late_start(overshoot_f);
        }

        // Releasing CUE stops playback and returns to cue point.
        SessionCommand::CuePreviewEnd { cue_point_sec, .. } => {
            let cue_point_sec = cue_point_sec.unwrap_or(deck.cue_point / sample_rate_f64);
            let frames = to_frames!(cue_point_sec);
            deck.is_playing = false;
            deck.is_cueing = false;
            deck.main_pos = frames;
            deck.cue_pos = frames;
        }
    }

    Ok(())
}

// Shared load body for DeckSnapshot and LoadTrack: full reset, then the
// samples from `load_samples` installed.
fn load_into(
    deck: &mut Deck,
    path: &str,
    sample_rate: u32,
    load_samples: &mut LoadSamples<'_>,
) -> Result<(), String> {
    let (samples, channels) = load_samples(path)?;
    let total_frames = samples.len() / channels;
    deck.reset();
    deck.samples = samples;
    deck.channels = channels;
    deck.device_sample_rate = sample_rate;
    deck.total_frames = total_frames;
    deck.duration = total_frames as f64 / sample_rate as f64;
    deck.loaded_path = Some(path.to_string());
    Ok(())
}

#[cfg(test)]
mod seconds_to_frames {
    use super::*;

    const SR: u32 = 44_100;
    const TOTAL: usize = 100_000;

    fn seeked_to(sec: f64, total_frames: usize) -> f64 {
        let mut deck = Deck::empty(SR);
        deck.total_frames = total_frames;
        apply_deck_command(
            &SessionCommand::Seek { deck: "A", sec },
            &mut deck,
            &mut ChannelStrip::new(SR as f32),
            SR,
            0.0,
            &mut |path: &str| Err(format!("no load: {path}")),
        )
        .expect("seek applies");
        deck.main_pos
    }

    #[test]
    fn a_second_is_the_sample_rate_in_frames() {
        assert!((seeked_to(1.0, TOTAL) - 44_100.0).abs() < 1e-9);
        assert!((seeked_to(0.5, TOTAL) - 22_050.0).abs() < 1e-9);
    }

    #[test]
    fn a_position_outside_the_track_lands_on_its_edge() {
        assert_eq!(seeked_to(-5.0, TOTAL), 0.0);
        assert_eq!(seeked_to(100.0, 44_100), 44_100.0);
    }
}
