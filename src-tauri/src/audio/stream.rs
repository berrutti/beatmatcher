use super::deck::{ChannelStrip, DeckState};
use super::dsp::LimiterState;
use cpal::traits::{DeviceTrait, HostTrait};
use std::collections::HashMap;
use std::sync::{atomic::Ordering, Arc, Mutex};

// ── cpal stream (Send wrapper) ────────────────────────────────────────────────
//
// cpal::Stream is !Send on some platforms (CoreAudio on macOS holds raw pointers).
// We wrap it to allow storage in Tauri managed state. The stream is never moved
// to another thread after creation; it's only kept alive by being held in AppAudio.

// cpal::Stream is !Send because some platform backends (CoreAudio, WASAPI) require
// the stream to be dropped on the thread that created it. We uphold this invariant
// manually: streams are only ever created and dropped inside rebuild_streams(), which
// is called from a single non-async context. The Mutex<Option<SendStream>> in AppAudio
// ensures no two threads can drop or replace a stream simultaneously.
#[allow(dead_code)]
pub(crate) struct SendStream(pub(crate) cpal::Stream);
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

type SharedDeck = Arc<Mutex<DeckState>>;
type SharedStrip = Arc<Mutex<ChannelStrip>>;

type ChannelPair = (SharedDeck, SharedStrip);
type ChannelPairs = Vec<ChannelPair>;

// -2 dBFS: gives the master bus headroom before the hardware clipping point.
// 10^(-2/20) = 0.7943...
pub(crate) const DEFAULT_MASTER_GAIN: f32 = 0.7943;

// ── Master monitor (metering + recording tap) ─────────────────────────────────
//
// Shared between AppAudio and every master stream callback via Arc clones.
// level_l/r are peak values from the last audio buffer, read by get_master_level.
// record_tx is None when not recording; the callback does a try_lock so it
// never blocks the audio thread.

#[derive(Clone)]
pub struct MasterMonitor {
    pub level_l: Arc<std::sync::atomic::AtomicU32>,
    pub level_r: Arc<std::sync::atomic::AtomicU32>,
    pub master_gain: Arc<std::sync::atomic::AtomicU32>,
    pub cue_mix: Arc<std::sync::atomic::AtomicU32>,
    pub limiter_enabled: Arc<std::sync::atomic::AtomicBool>,
    pub record_tx: Arc<Mutex<Option<std::sync::mpsc::SyncSender<Vec<f32>>>>>,
    // Free-running count of master output frames produced by the audio device.
    // It is the soundcard's own clock; session playback schedules events against
    // it instead of wall-clock time so replay stays locked to the audio output.
    pub output_frames: Arc<std::sync::atomic::AtomicU64>,
}

impl MasterMonitor {
    pub(crate) fn new() -> Self {
        Self {
            level_l: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            level_r: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            master_gain: Arc::new(std::sync::atomic::AtomicU32::new(
                DEFAULT_MASTER_GAIN.to_bits(),
            )),
            cue_mix: Arc::new(std::sync::atomic::AtomicU32::new(0u32)),
            limiter_enabled: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            record_tx: Arc::new(Mutex::new(None)),
            output_frames: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    pub fn output_frames(&self) -> u64 {
        self.output_frames.load(Ordering::Relaxed)
    }

    pub fn get_levels(&self) -> [f32; 2] {
        [
            f32::from_bits(self.level_l.load(Ordering::Relaxed)),
            f32::from_bits(self.level_r.load(Ordering::Relaxed)),
        ]
    }

    pub fn set_master_gain(&self, gain: f32) {
        self.master_gain
            .store(gain.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_cue_mix(&self, mix: f32) {
        self.cue_mix
            .store(mix.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_limiter_enabled(&self, enabled: bool) {
        self.limiter_enabled.store(enabled, Ordering::Relaxed);
    }

    pub(crate) fn store_levels(&self, l: f32, r: f32) {
        self.level_l.store(l.to_bits(), Ordering::Relaxed);
        self.level_r.store(r.to_bits(), Ordering::Relaxed);
    }
}

// Build a paired list of (deck, strip) in a consistent order for use in stream callbacks.
pub(crate) fn channel_pairs(
    decks: &HashMap<String, SharedDeck>,
    strips: &HashMap<String, SharedStrip>,
) -> ChannelPairs {
    let mut ids: Vec<&String> = decks.keys().collect();
    ids.sort();
    ids.into_iter()
        .filter_map(|id| {
            let deck = decks.get(id)?;
            let strip = strips.get(id)?;
            Some((Arc::clone(deck), Arc::clone(strip)))
        })
        .collect()
}

pub(crate) fn find_output_device(device_id: &str) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    host.devices()
        .map_err(|e| e.to_string())?
        .filter(|d| {
            d.supported_output_configs()
                .map(|mut c| c.next().is_some())
                .unwrap_or(false)
        })
        .find(|d| d.name().map(|n| n == device_id).unwrap_or(false))
        .ok_or_else(|| format!("device not found: {}", device_id))
}

// Find the supported output config with the fewest channels that still satisfies
// min_channels, preferring configs whose sample-rate range includes preferred_sr.
pub(crate) fn best_output_config(
    device: &cpal::Device,
    min_channels: usize,
    preferred_sr: u32,
) -> Result<cpal::SupportedStreamConfig, String> {
    let min_ch = min_channels as u16;
    let target_sr = cpal::SampleRate(preferred_sr);

    let all: Vec<_> = device
        .supported_output_configs()
        .map_err(|e| e.to_string())?
        .collect();

    log::info!(
        "best_output_config: device='{}' min_channels={} preferred_sr={} | supported=[{}]",
        device.name().unwrap_or_default(),
        min_channels,
        preferred_sr,
        all.iter()
            .map(|c| format!(
                "{}ch/{}-{}Hz",
                c.channels(),
                c.min_sample_rate().0,
                c.max_sample_rate().0
            ))
            .collect::<Vec<_>>()
            .join(", ")
    );

    // Prefer configs that include the current sample rate so loaded tracks play
    // at the right pitch. Fall back to any config with enough channels.
    if let Some(range) = all
        .iter()
        .filter(|c| {
            c.channels() >= min_ch
                && c.min_sample_rate() <= target_sr
                && c.max_sample_rate() >= target_sr
        })
        .min_by_key(|c| c.channels())
    {
        let cfg = (*range).with_sample_rate(target_sr);
        log::info!(
            "best_output_config: chose {}ch @ {}Hz (sr match)",
            cfg.channels(),
            cfg.sample_rate().0
        );
        return Ok(cfg);
    }

    if let Some(range) = all
        .iter()
        .filter(|c| c.channels() >= min_ch)
        .min_by_key(|c| c.channels())
    {
        let cfg = (*range).with_max_sample_rate();
        log::info!(
            "best_output_config: chose {}ch @ {}Hz (no sr match)",
            cfg.channels(),
            cfg.sample_rate().0
        );
        return Ok(cfg);
    }

    // Device has no config with enough channels. Fall back to default and let
    // mix_frame clamp gracefully (audio will be silent for out-of-range offsets).
    let cfg = device.default_output_config().map_err(|e| e.to_string())?;
    log::warn!(
        "best_output_config: no config with >={} channels, falling back to default ({}ch)",
        min_channels,
        cfg.channels()
    );
    Ok(cfg)
}

#[inline]
fn f32_to_i16_sample(s: f32) -> i16 {
    (s * i16::MAX as f32) as i16
}

// Dispatch over sample format once. The caller provides a closure that fills a
// float buffer; the I16 branch transparently routes through an intermediate buffer.
fn build_float_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    stream_config: cpal::StreamConfig,
    mut fill: impl FnMut(&mut [f32]) + Send + 'static,
) -> Result<cpal::Stream, Box<dyn std::error::Error + Send + Sync>> {
    match config.sample_format() {
        cpal::SampleFormat::F32 => Ok(device.build_output_stream(
            &stream_config,
            move |data: &mut [f32], _| fill(data),
            |e| eprintln!("audio stream error: {:?}", e),
            None,
        )?),
        cpal::SampleFormat::I16 => {
            let mut buf: Vec<f32> = Vec::new();
            Ok(device.build_output_stream(
                &stream_config,
                move |data: &mut [i16], _| {
                    buf.resize(data.len(), 0.0);
                    fill(&mut buf);
                    for (d, s) in data.iter_mut().zip(buf.iter()) {
                        *d = f32_to_i16_sample(*s);
                    }
                },
                |e| eprintln!("audio stream error: {:?}", e),
                None,
            )?)
        }
        fmt => Err(format!("unsupported sample format: {:?}", fmt).into()),
    }
}

pub(crate) fn build_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    channels: ChannelPairs,
    is_cue: bool,
    channel_offset: usize,
    monitor: Option<MasterMonitor>,
    buffer_frames: u32,
) -> Result<cpal::Stream, Box<dyn std::error::Error + Send + Sync>> {
    let mut stream_config: cpal::StreamConfig = config.clone().into();
    if buffer_frames > 0 {
        stream_config.buffer_size = cpal::BufferSize::Fixed(buffer_frames);
    }
    let output_channels = config.channels() as usize;
    let label = if is_cue { "cue" } else { "master" };
    log::info!(
        "build_stream [{}]: output_channels={} channel_offset={} format={:?} sample_rate={}",
        label,
        output_channels,
        channel_offset,
        config.sample_format(),
        config.sample_rate().0
    );
    let mut limiter = LimiterState::new(config.sample_rate().0 as f32);
    build_float_stream(device, config, stream_config, move |data| {
        fill_output(
            data,
            output_channels,
            &channels,
            is_cue,
            channel_offset,
            monitor.as_ref(),
            &mut limiter,
        );
    })
}

pub(crate) fn build_cue_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    channels: ChannelPairs,
    channel_offset: usize,
    monitor: Option<MasterMonitor>,
    buffer_frames: u32,
) -> Result<cpal::Stream, Box<dyn std::error::Error + Send + Sync>> {
    let mut stream_config: cpal::StreamConfig = config.clone().into();
    if buffer_frames > 0 {
        stream_config.buffer_size = cpal::BufferSize::Fixed(buffer_frames);
    }
    let output_channels = config.channels() as usize;
    log::info!(
        "build_cue_stream: output_channels={} channel_offset={} master_tap={} format={:?} sr={}",
        output_channels,
        channel_offset,
        monitor.is_some(),
        config.sample_format(),
        config.sample_rate().0
    );
    let mut master_mix: Vec<f32> = Vec::new();
    let mut cue_buf: Vec<f32> = Vec::new();
    let mut limiter = LimiterState::new(config.sample_rate().0 as f32);
    build_float_stream(device, config, stream_config, move |data| match &monitor {
        Some(m) => fill_cue_with_master_tap(
            data,
            output_channels,
            &channels,
            channel_offset,
            m,
            &mut master_mix,
            &mut cue_buf,
            &mut limiter,
        ),
        None => fill_output(
            data,
            output_channels,
            &channels,
            true,
            channel_offset,
            None,
            &mut limiter,
        ),
    })
}

struct CombinedMixContext<'a> {
    channels: &'a ChannelPairs,
    main_offset: usize,
    cue_offset: usize,
    monitor: &'a MasterMonitor,
    cue_buf: &'a mut Vec<f32>,
    limiter: &'a mut LimiterState,
}

pub(crate) struct CombinedStreamParams {
    pub(crate) channels: ChannelPairs,
    pub(crate) output_channels: usize,
    pub(crate) main_offset: usize,
    pub(crate) cue_offset: usize,
    pub(crate) monitor: MasterMonitor,
    pub(crate) buffer_frames: u32,
}

pub(crate) fn build_combined_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    params: CombinedStreamParams,
) -> Result<cpal::Stream, Box<dyn std::error::Error + Send + Sync>> {
    let CombinedStreamParams {
        channels,
        output_channels,
        main_offset,
        cue_offset,
        monitor,
        buffer_frames,
    } = params;

    let mut stream_config: cpal::StreamConfig = config.clone().into();
    if buffer_frames > 0 {
        stream_config.buffer_size = cpal::BufferSize::Fixed(buffer_frames);
    }
    log::info!(
        "build_combined_stream: output_channels={} main_offset={} cue_offset={} format={:?} sr={}",
        output_channels,
        main_offset,
        cue_offset,
        config.sample_format(),
        config.sample_rate().0
    );
    let mut cue_buf: Vec<f32> = Vec::new();
    let mut limiter = LimiterState::new(config.sample_rate().0 as f32);
    build_float_stream(device, config, stream_config, move |data| {
        let mut ctx = CombinedMixContext {
            channels: &channels,
            main_offset,
            cue_offset,
            monitor: &monitor,
            cue_buf: &mut cue_buf,
            limiter: &mut limiter,
        };
        fill_output_combined(data, output_channels, &mut ctx);
    })
}

fn fill_output(
    data: &mut [f32],
    output_channels: usize,
    channels: &[ChannelPair],
    is_cue: bool,
    channel_offset: usize,
    monitor: Option<&MasterMonitor>,
    limiter: &mut LimiterState,
) {
    data.fill(0.0);
    let frames = data.len() / output_channels.max(1);
    let deck_tick: fn(&mut DeckState) -> (f32, f32) = if is_cue {
        DeckState::cue_tick
    } else {
        DeckState::main_tick
    };
    let strip_process: fn(&mut ChannelStrip, f32, f32) -> (f32, f32) = if is_cue {
        ChannelStrip::process_cue
    } else {
        ChannelStrip::process_main
    };

    for (deck_arc, strip_arc) in channels {
        let mut deck = deck_arc.lock().expect("deck mutex poisoned");
        let mut strip = strip_arc.lock().expect("channel strip mutex poisoned");
        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;
        for i in 0..frames {
            let (raw_l, raw_r) = deck_tick(&mut deck);
            let (l, r) = strip_process(&mut strip, raw_l, raw_r);
            if !is_cue {
                sum_l += l.abs();
                sum_r += r.abs();
            }
            mix_frame(data, i, output_channels, channel_offset, l, r);
        }
        if !is_cue {
            strip.store_level(sum_l / frames as f32, sum_r / frames as f32);
        }
    }

    if let Some(m) = monitor {
        let gain = f32::from_bits(m.master_gain.load(Ordering::Relaxed));
        let use_limiter = m.limiter_enabled.load(Ordering::Relaxed);
        for i in 0..frames {
            let base = i * output_channels + channel_offset;
            if base + 1 < data.len() {
                let l = data[base] * gain;
                let r = data[base + 1] * gain;
                let (l, r) = if use_limiter {
                    limiter.process(l, r)
                } else {
                    (l.clamp(-1.0, 1.0), r.clamp(-1.0, 1.0))
                };
                data[base] = l;
                data[base + 1] = r;
            }
        }
        tap_master_output(data, frames, output_channels, channel_offset, m);
    } else {
        for sample in data.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }
}

// Used when main output is unrouted but a cue stream is active and recording is
// requested. Renders both the cue signal (into `data`) and the master mix (into
// a temporary stereo buffer), then taps the master mix for metering/recording.
// Nothing from the master mix is sent to the cue hardware output.
#[allow(clippy::too_many_arguments)]
fn fill_cue_with_master_tap(
    data: &mut [f32],
    output_channels: usize,
    channels: &[ChannelPair],
    cue_offset: usize,
    monitor: &MasterMonitor,
    master_mix: &mut Vec<f32>,
    cue_buf: &mut Vec<f32>,
    limiter: &mut LimiterState,
) {
    data.fill(0.0);
    let frames = data.len() / output_channels.max(1);
    master_mix.resize(frames * 2, 0.0);
    master_mix.fill(0.0);
    cue_buf.resize(frames * 2, 0.0);
    cue_buf.fill(0.0);

    for (deck_arc, strip_arc) in channels {
        let mut deck = deck_arc.lock().expect("deck mutex poisoned");
        let mut strip = strip_arc.lock().expect("channel strip mutex poisoned");
        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;
        for i in 0..frames {
            let (l, r) = deck.main_tick();
            let (ml, mr) = strip.process_main(l, r);
            sum_l += ml.abs();
            sum_r += mr.abs();
            master_mix[i * 2] += ml;
            master_mix[i * 2 + 1] += mr;

            let (l, r) = deck.cue_tick();
            let (cl, cr) = strip.process_cue(l, r);
            cue_buf[i * 2] += cl;
            cue_buf[i * 2 + 1] += cr;
        }
        strip.store_level(sum_l / frames as f32, sum_r / frames as f32);
    }

    let gain = f32::from_bits(monitor.master_gain.load(Ordering::Relaxed));
    let use_limiter = monitor.limiter_enabled.load(Ordering::Relaxed);
    for i in 0..frames {
        let l = master_mix[i * 2] * gain;
        let r = master_mix[i * 2 + 1] * gain;
        let (l, r) = if use_limiter {
            limiter.process(l, r)
        } else {
            (l.clamp(-1.0, 1.0), r.clamp(-1.0, 1.0))
        };
        master_mix[i * 2] = l;
        master_mix[i * 2 + 1] = r;
    }
    let mix = f32::from_bits(monitor.cue_mix.load(Ordering::Relaxed));
    for i in 0..frames {
        let cl = cue_buf[i * 2].clamp(-1.0, 1.0);
        let cr = cue_buf[i * 2 + 1].clamp(-1.0, 1.0);
        let ml = master_mix[i * 2];
        let mr = master_mix[i * 2 + 1];
        let out_l = cl * (1.0 - mix) + ml * mix;
        let out_r = cr * (1.0 - mix) + mr * mix;
        mix_frame(data, i, output_channels, cue_offset, out_l, out_r);
    }
    tap_master_output(master_mix, frames, 2, 0, monitor);
}

#[inline]
fn mix_frame(
    data: &mut [f32],
    frame: usize,
    channels: usize,
    channel_offset: usize,
    l: f32,
    r: f32,
) {
    let base = frame * channels + channel_offset;
    let remaining = channels.saturating_sub(channel_offset);
    if remaining == 0 {
        return;
    }
    if remaining == 1 {
        if base < data.len() {
            data[base] += (l + r) * 0.5;
        }
    } else if base + 1 < data.len() {
        data[base] += l;
        data[base + 1] += r;
    }
}

fn fill_output_combined(
    data: &mut [f32],
    output_channels: usize,
    ctx: &mut CombinedMixContext<'_>,
) {
    data.fill(0.0);

    let frames = data.len() / output_channels.max(1);

    ctx.cue_buf.resize(frames * 2, 0.0);
    ctx.cue_buf.fill(0.0);

    for (deck_arc, strip_arc) in ctx.channels {
        let mut deck = deck_arc.lock().expect("deck mutex poisoned");
        let mut strip = strip_arc.lock().expect("channel strip mutex poisoned");

        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;

        for i in 0..frames {
            let (l, r) = deck.main_tick();
            let (ml, mr) = strip.process_main(l, r);

            sum_l += ml.abs();
            sum_r += mr.abs();

            mix_frame(data, i, output_channels, ctx.main_offset, ml, mr);

            let (l, r) = deck.cue_tick();
            let (cl, cr) = strip.process_cue(l, r);

            ctx.cue_buf[i * 2] += cl;
            ctx.cue_buf[i * 2 + 1] += cr;
        }

        strip.store_level(sum_l / frames as f32, sum_r / frames as f32);
    }

    let gain = f32::from_bits(ctx.monitor.master_gain.load(Ordering::Relaxed));

    let use_limiter = ctx.monitor.limiter_enabled.load(Ordering::Relaxed);

    for i in 0..frames {
        let idx = i * output_channels + ctx.main_offset;

        if idx + 1 < data.len() {
            let l = data[idx] * gain;
            let r = data[idx + 1] * gain;

            let (l, r) = if use_limiter {
                ctx.limiter.process(l, r)
            } else {
                (l.clamp(-1.0, 1.0), r.clamp(-1.0, 1.0))
            };

            data[idx] = l;
            data[idx + 1] = r;
        }
    }

    let mix = f32::from_bits(ctx.monitor.cue_mix.load(Ordering::Relaxed));

    for i in 0..frames {
        let cl = ctx.cue_buf[i * 2].clamp(-1.0, 1.0);
        let cr = ctx.cue_buf[i * 2 + 1].clamp(-1.0, 1.0);

        let main_idx = i * output_channels + ctx.main_offset;

        let ml = if main_idx + 1 < data.len() {
            data[main_idx]
        } else {
            0.0
        };

        let mr = if main_idx + 1 < data.len() {
            data[main_idx + 1]
        } else {
            0.0
        };

        let out_l = cl * (1.0 - mix) + ml * mix;
        let out_r = cr * (1.0 - mix) + mr * mix;

        mix_frame(data, i, output_channels, ctx.cue_offset, out_l, out_r);
    }

    tap_master_output(data, frames, output_channels, ctx.main_offset, ctx.monitor);
}

// Reads the final clamped master L/R samples from the output buffer, stores
// the peak level in the monitor atomics, and forwards to the recording channel
// if recording is active. Uses try_lock so the audio callback never blocks.
fn tap_master_output(
    data: &[f32],
    frames: usize,
    output_channels: usize,
    channel_offset: usize,
    monitor: &MasterMonitor,
) {
    let mut sum_l = 0.0f32;
    let mut sum_r = 0.0f32;
    let mut counted = 0usize;
    for i in 0..frames {
        let base = i * output_channels + channel_offset;
        if base + 1 < data.len() {
            sum_l += data[base].abs();
            sum_r += data[base + 1].abs();
            counted += 1;
        }
    }
    let n = counted.max(1) as f32;
    monitor.store_levels(sum_l / n, sum_r / n);

    // Advance the master output frame clock. Runs once per master buffer in
    // every routing mode (main, combined, cue-with-master-tap).
    monitor
        .output_frames
        .fetch_add(frames as u64, Ordering::Relaxed);

    if let Ok(guard) = monitor.record_tx.try_lock() {
        if let Some(ref tx) = *guard {
            let mut chunk = Vec::with_capacity(frames * 2);
            for i in 0..frames {
                let base = i * output_channels + channel_offset;
                if base + 1 < data.len() {
                    chunk.push(data[base]);
                    chunk.push(data[base + 1]);
                }
            }
            let _ = tx.try_send(chunk);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- f32_to_i16_sample ---

    #[test]
    fn f32_to_i16_sample_full_scale() {
        assert_eq!(f32_to_i16_sample(1.0), i16::MAX);
        // -1.0 × 32767 = -32767, which fits in i16
        assert_eq!(f32_to_i16_sample(-1.0), -i16::MAX);
    }

    #[test]
    fn f32_to_i16_sample_zero() {
        assert_eq!(f32_to_i16_sample(0.0), 0);
    }

    #[test]
    fn f32_to_i16_sample_midpoint() {
        let result = f32_to_i16_sample(0.5);
        let expected = (0.5 * i16::MAX as f32) as i16;
        assert_eq!(result, expected);
    }

    // --- mix_frame ---

    #[test]
    fn mix_frame_writes_stereo() {
        let mut buf = vec![0.0f32; 4];
        mix_frame(&mut buf, 0, 2, 0, 0.5, -0.5);
        assert_eq!(buf[0], 0.5);
        assert_eq!(buf[1], -0.5);
        assert_eq!(buf[2], 0.0);
        assert_eq!(buf[3], 0.0);
    }

    #[test]
    fn mix_frame_accumulates() {
        let mut buf = vec![0.4f32, 0.3, 0.0, 0.0];
        mix_frame(&mut buf, 0, 2, 0, 0.1, 0.2);
        assert!((buf[0] - 0.5).abs() < 1e-6, "buf[0]={}", buf[0]);
        assert!((buf[1] - 0.5).abs() < 1e-6, "buf[1]={}", buf[1]);
    }

    #[test]
    fn mix_frame_mono_downmix() {
        let mut buf = vec![0.0f32; 2];
        mix_frame(&mut buf, 0, 1, 0, 0.4, 0.8);
        assert!((buf[0] - 0.6).abs() < 1e-6, "buf[0]={}", buf[0]);
    }

    #[test]
    fn mix_frame_out_of_bounds_is_silent() {
        let mut buf = vec![0.0f32; 2];
        mix_frame(&mut buf, 0, 2, 2, 1.0, 1.0);
        assert_eq!(buf[0], 0.0);
        assert_eq!(buf[1], 0.0);
    }
}
