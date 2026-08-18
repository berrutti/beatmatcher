use super::dsp::Biquad;

const BAND_BASS_HZ: f32 = 250.0;
const BAND_MID_HZ: f32 = 2_000.0;

pub fn compute_spectral_bands(
    samples: &[f32],
    channels: usize,
    sample_rate: u32,
) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    if samples.is_empty() || channels == 0 {
        return (Vec::new(), Vec::new(), Vec::new());
    }
    let n = samples.len() / channels;
    let sr = sample_rate as f32;
    let butterworth_q = 1.0 / std::f32::consts::SQRT_2;

    let mut lp_bass = Biquad::low_pass(sr, BAND_BASS_HZ, butterworth_q);
    let mut lp_bass_mid = Biquad::low_pass(sr, BAND_MID_HZ, butterworth_q);

    let mut bass = Vec::with_capacity(n);
    let mut mid = Vec::with_capacity(n);
    let mut high = Vec::with_capacity(n);

    for frame in 0..n {
        let mono = if channels == 1 {
            samples[frame]
        } else {
            let sum: f32 = (0..channels).map(|ch| samples[frame * channels + ch]).sum();
            sum / channels as f32
        };
        let b = lp_bass.process(mono);
        let bm = lp_bass_mid.process(mono);
        bass.push(b);
        mid.push(bm - b);
        high.push(mono - bm);
    }

    (bass, mid, high)
}

// RMS rather than peak: mastered music puts a near-|1.0| sample in almost every bin, so
// peak heights collapse to a flat block. Channels ride band energy un-normalised, and the
// amplitude is stored raw so the sqrt curve in the frontend has range left to spread.
const AMP_DISPLAY_BOOST: f32 = 1.0;
const BAND_DISPLAY_BOOST: f32 = 1.0;

pub fn compute_spectral_waveform_region(
    samples: &[f32],
    channels: usize,
    bands: &super::SpectralBands,
    sample_rate: u32,
    start_sec: f64,
    end_sec: f64,
    num_points: usize,
) -> Vec<f32> {
    let (bass, mid, high) = (&bands.bass, &bands.mid, &bands.high);
    let (bass_scale, mid_scale, high_scale) = (bands.bass_scale, bands.mid_scale, bands.high_scale);
    if bass.is_empty() || num_points == 0 {
        return vec![0.0; num_points * 4];
    }
    let total_frames = bass.len();
    let sr = sample_rate as f64;
    let start_frame = (start_sec * sr).max(0.0) as usize;
    let end_frame = ((end_sec * sr) as usize).min(total_frames);

    if start_frame >= end_frame {
        return vec![0.0; num_points * 4];
    }

    let visible_frames = end_frame - start_frame;
    let frames_per_point = visible_frames as f64 / num_points as f64;

    let mut result = Vec::with_capacity(num_points * 4);

    for point_index in 0..num_points {
        let bin_start = start_frame + (point_index as f64 * frames_per_point) as usize;
        let bin_end = (start_frame + ((point_index + 1) as f64 * frames_per_point) as usize)
            .min(end_frame)
            .max(bin_start + 1);

        let mut sum_bass_sq = 0.0f32;
        let mut sum_mid_sq = 0.0f32;
        let mut sum_high_sq = 0.0f32;
        let mut sum_sample_sq = 0.0f32;
        let count = (bin_end - bin_start) as f32;

        for frame in bin_start..bin_end {
            sum_bass_sq += bass[frame] * bass[frame];
            sum_mid_sq += mid[frame] * mid[frame];
            sum_high_sq += high[frame] * high[frame];
            for ch in 0..channels {
                let s = samples[frame * channels + ch];
                sum_sample_sq += s * s;
            }
        }

        let rms_amp = (sum_sample_sq / (count * channels as f32)).sqrt();
        let rms_bass = (sum_bass_sq / count).sqrt();
        let rms_mid = (sum_mid_sq / count).sqrt();
        let rms_high = (sum_high_sq / count).sqrt();

        let r = (rms_bass * bass_scale * BAND_DISPLAY_BOOST).min(1.0);
        let g = (rms_mid * mid_scale * BAND_DISPLAY_BOOST).min(1.0);
        let b = (rms_high * high_scale * BAND_DISPLAY_BOOST).min(1.0);
        let amp = (rms_amp * AMP_DISPLAY_BOOST).min(1.0);

        result.push(r);
        result.push(g);
        result.push(b);
        result.push(amp);
    }

    result
}

// RMS amplitude over the frame sub-range [start_frame, end_frame), binned into
// num_points. Used for zoom-driven LOD: fetch only the visible track region at
// roughly one point per on-screen pixel, so detail scales with zoom regardless
// of track length.
pub fn compute_amplitude_region(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
    num_points: usize,
) -> Vec<f32> {
    if samples.is_empty() || channels == 0 || num_points == 0 {
        return vec![0.0; num_points];
    }
    let total_frames = samples.len() / channels;
    let start = start_frame.min(total_frames);
    let end = end_frame.max(start + 1);
    // frames_per_point is based on the requested (possibly past-track-end) span,
    // not the real track length, so a bin's position stays proportional to what
    // the caller asked for. Bins that fall beyond total_frames are left silent
    // instead of stretching the real audio to fill every point.
    let frames_per_point = (end - start) as f64 / num_points as f64;
    let mut result = Vec::with_capacity(num_points);
    for i in 0..num_points {
        let bin_start = (start as f64 + i as f64 * frames_per_point) as usize;
        let bin_end = (start as f64 + (i + 1) as f64 * frames_per_point) as usize;
        let bin_start = bin_start.min(total_frames);
        let bin_end = bin_end.min(total_frames).max(bin_start);
        if bin_start >= bin_end {
            result.push(0.0);
            continue;
        }
        let sample_count = ((bin_end - bin_start) * channels) as f32;
        let sum_of_squares: f32 = samples[bin_start * channels..bin_end * channels]
            .iter()
            .map(|s| s * s)
            .sum();
        result.push((sum_of_squares / sample_count).sqrt().min(1.0));
    }
    result
}

// Isolate kick drum energy for onset detection. Bass drum fundamentals sit
// between 60-150 Hz; cutting above 150 Hz removes mid/snare content that
// would create false beat intervals.

const BPM_LOWPASS_HZ: f32 = 150.0;
pub(crate) const BPM_MIN: f64 = 90.0;
pub(crate) const BPM_MAX: f64 = 180.0;
const PEAK_SKIP_SAMPLES: usize = 10_000;
const NEIGHBOR_COUNT: usize = 10;
const CLUSTER_TOLERANCE: f64 = 1.0;
const THRESHOLDS: &[f32] = &[0.9, 0.8, 0.7];
const MIN_PEAKS: usize = 15;

// 2nd-order Butterworth lowpass matching Web Audio BiquadFilterNode (type='lowpass', default Q=1/sqrt(2)).
fn lowpass_biquad(input: &[f32], sample_rate: u32, cutoff_hz: f32) -> Vec<f32> {
    use std::f64::consts::PI;
    let w0 = 2.0 * PI * cutoff_hz as f64 / sample_rate as f64;
    let cos_w0 = w0.cos();
    // alpha = sin(w0) / (2*Q), Q = 1/sqrt(2)  =>  alpha = sin(w0) / sqrt(2)
    let alpha = w0.sin() / std::f64::consts::SQRT_2;
    let b0 = (1.0 - cos_w0) / 2.0;
    let b1 = 1.0 - cos_w0;
    let b2 = (1.0 - cos_w0) / 2.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha;
    let mut output = Vec::with_capacity(input.len());
    let mut x1 = 0.0f64;
    let mut x2 = 0.0f64;
    let mut y1 = 0.0f64;
    let mut y2 = 0.0f64;
    for &in_sample in input {
        let x0 = in_sample as f64;
        let y = (b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2) / a0;
        output.push(y as f32);
        x2 = x1;
        x1 = x0;
        y2 = y1;
        y1 = y;
    }
    output
}

fn find_peaks(data: &[f32], threshold: f32, skip: usize) -> Vec<usize> {
    let mut peaks = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if data[i].abs() > threshold {
            peaks.push(i);
            i += skip;
        }
        i += 1;
    }
    peaks
}

fn interval_to_bpm(interval: usize, sample_rate: u32, bpm_min: f64, bpm_max: f64) -> Option<f64> {
    if interval == 0 {
        return None;
    }
    let mut bpm = 60.0 * sample_rate as f64 / interval as f64;
    while bpm < bpm_min {
        bpm *= 2.0;
    }
    while bpm > bpm_max {
        bpm /= 2.0;
    }
    if bpm >= bpm_min && bpm <= bpm_max {
        Some(bpm)
    } else {
        None
    }
}

struct BpmCluster {
    weighted_bpm_sum: f64,
    count: usize,
}

pub fn detect_bpm(mono: &[f32], sample_rate: u32, bpm_min: f64, bpm_max: f64) -> Option<f64> {
    let filtered = lowpass_biquad(mono, sample_rate, BPM_LOWPASS_HZ);

    let mut peaks = Vec::new();
    for &threshold in THRESHOLDS {
        peaks = find_peaks(&filtered, threshold, PEAK_SKIP_SAMPLES);
        if peaks.len() >= MIN_PEAKS {
            break;
        }
    }

    if peaks.len() < 2 {
        return None;
    }

    let mut interval_counts: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::new();
    for i in 0..peaks.len() {
        let limit = (i + NEIGHBOR_COUNT + 1).min(peaks.len());
        for j in (i + 1)..limit {
            let interval = peaks[j] - peaks[i];
            *interval_counts.entry(interval).or_insert(0) += 1;
        }
    }

    // For intervals whose raw BPM falls below BPM_MIN, the energy peaks are landing
    // only on every other beat. Add synthetic votes for interval/2 so the actual beat
    // period becomes visible to the clusterer. Only /2. Dividing by 3 would introduce
    // spurious fractional-BPM candidates for common syncopated patterns.
    let long_intervals: Vec<(usize, usize)> = interval_counts
        .iter()
        .filter(|(&interval, _)| {
            interval > 0 && 60.0 * sample_rate as f64 / (interval as f64) < bpm_min
        })
        .map(|(&k, &v)| (k, v))
        .collect();
    for (interval, count) in long_intervals {
        if interval % 2 == 0 {
            *interval_counts.entry(interval / 2).or_insert(0) += count;
        }
    }

    let mut clusters: Vec<BpmCluster> = Vec::new();

    for (&interval, &count) in &interval_counts {
        if let Some(bpm) = interval_to_bpm(interval, sample_rate, bpm_min, bpm_max) {
            let mut merged = false;
            for cluster in &mut clusters {
                let cluster_avg = cluster.weighted_bpm_sum / cluster.count as f64;
                if (cluster_avg - bpm).abs() <= CLUSTER_TOLERANCE {
                    cluster.weighted_bpm_sum += bpm * count as f64;
                    cluster.count += count;
                    merged = true;
                    break;
                }
            }
            if !merged {
                clusters.push(BpmCluster {
                    weighted_bpm_sum: bpm * count as f64,
                    count,
                });
            }
        }
    }

    // Sort by: most votes first, then most-integer BPM (97.3 loses to 146), then higher BPM.
    clusters.sort_by(|a, b| {
        let bpm_a = a.weighted_bpm_sum / a.count as f64;
        let bpm_b = b.weighted_bpm_sum / b.count as f64;
        let frac_a = (bpm_a - bpm_a.round()).abs();
        let frac_b = (bpm_b - bpm_b.round()).abs();
        b.count
            .cmp(&a.count)
            .then_with(|| {
                frac_a
                    .partial_cmp(&frac_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                bpm_b
                    .partial_cmp(&bpm_a)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    let result = clusters.first().map(|cluster| {
        let bpm = cluster.weighted_bpm_sum / cluster.count as f64;
        (bpm * 10.0).round() / 10.0
    });
    log::info!(
        "detect_bpm: peaks={} clusters={} result={:?}",
        peaks.len(),
        clusters.len(),
        result
    );
    result
}

pub fn detect_silence_end(mono: &[f32], sample_rate: u32) -> f64 {
    const THRESHOLD: f32 = 0.01;
    const WINDOW_MS: usize = 50;
    let window_frames = (sample_rate as usize * WINDOW_MS / 1000).max(1);

    let mut frame = 0;
    while frame + window_frames <= mono.len() {
        let rms = (mono[frame..frame + window_frames]
            .iter()
            .map(|&x| x * x)
            .sum::<f32>()
            / window_frames as f32)
            .sqrt();

        if rms > THRESHOLD {
            let silence_end_secs = frame as f64 / sample_rate as f64;
            log::info!(
                "detect_silence_end: audio starts at {:.3}s (frame {}, rms={:.5})",
                silence_end_secs,
                frame,
                rms
            );
            return silence_end_secs;
        }

        frame += window_frames;
    }

    log::info!(
        "detect_silence_end: no audio above threshold {:.4} found in {} samples, returning 0.0",
        THRESHOLD,
        mono.len()
    );
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_wave(freq_hz: f32, sample_rate: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq_hz * i as f32 / sample_rate).sin())
            .collect()
    }

    #[test]
    fn compute_amplitude_region_does_not_compress_past_track_end() {
        let total_frames = 100;
        let mut samples = vec![0.0f32; total_frames];
        // Loud content at frames [40, 50), the true track's back half.
        for sample in samples.iter_mut().take(50).skip(40) {
            *sample = 1.0;
        }
        // Request spans [0, 200) frames: double the real track length, as
        // happens when zoom padding overshoots the end of the track.
        let result = compute_amplitude_region(&samples, 1, 0, 200, 10);

        // Correct proportional position: 40/200 = 0.20 -> bin 2, 50/200 = 0.25 -> bin 2.
        assert!(
            result[2] > 0.5,
            "expected loud bin at index 2, got {:?}",
            result
        );
        // Buggy (pre-fix) behavior compresses the real track into bins 4-5
        // (40/100 -> bin 4, 50/100 -> bin 5); those must stay silent here.
        assert!(
            result[4] < 0.1,
            "index 4 should be silent, got {:?}",
            result
        );
        assert!(
            result[5] < 0.1,
            "index 5 should be silent, got {:?}",
            result
        );
    }

    #[test]
    fn interval_to_bpm_zero_is_none() {
        assert!(interval_to_bpm(0, 44100, BPM_MIN, BPM_MAX).is_none());
    }

    #[test]
    fn interval_to_bpm_folds_into_range() {
        // 44100 samples at 44100 Hz = 1 beat/sec = 60 BPM. Below BPM_MIN (90)
        // so doubles to 120, which is within [90, 180].
        let bpm = interval_to_bpm(44100, 44100, BPM_MIN, BPM_MAX).expect("should be Some");
        assert!((bpm - 120.0).abs() < 0.5, "expected ~120 BPM, got {}", bpm);
    }

    #[test]
    fn interval_to_bpm_direct_hit() {
        // 60 * 44100 / interval = 128 => interval = 60 * 44100 / 128 = 20671.875
        let interval = (60.0_f64 * 44100.0 / 128.0).round() as usize;
        let bpm = interval_to_bpm(interval, 44100, BPM_MIN, BPM_MAX).expect("should be Some");
        assert!((bpm - 128.0).abs() < 1.0, "expected ~128 BPM, got {}", bpm);
    }

    #[test]
    fn find_peaks_detects_isolated_spikes() {
        let mut signal = vec![0.0f32; 1000];
        signal[100] = 1.0;
        signal[400] = 1.0;
        signal[700] = 1.0;
        let peaks = find_peaks(&signal, 0.5, 200);
        assert_eq!(peaks, vec![100, 400, 700]);
    }

    #[test]
    fn find_peaks_skip_prevents_nearby_detection() {
        let mut signal = vec![0.0f32; 500];
        signal[100] = 1.0;
        signal[150] = 1.0;
        let peaks = find_peaks(&signal, 0.5, 200);
        assert_eq!(peaks.len(), 1);
        assert_eq!(peaks[0], 100);
    }

    #[test]
    fn detect_silence_end_all_zeros_returns_zero() {
        let silence = vec![0.0f32; 44100];
        assert_eq!(detect_silence_end(&silence, 44100), 0.0);
    }

    #[test]
    fn detect_silence_end_loud_signal_returns_near_zero() {
        let loud = sine_wave(440.0, 44100.0, 44100);
        let result = detect_silence_end(&loud, 44100);
        assert!(result < 0.1, "expected ~0.0s, got {}s", result);
    }

    #[test]
    fn detect_silence_end_locates_audio_start() {
        let sr = 44100u32;
        let silence_sec = 0.5;
        let silence_frames = (silence_sec * sr as f64) as usize;
        let mut signal = vec![0.0f32; sr as usize];
        let tone = sine_wave(440.0, sr as f32, signal.len() - silence_frames);
        signal[silence_frames..].copy_from_slice(&tone);
        let result = detect_silence_end(&signal, sr);
        assert!(
            result >= 0.4 && result <= 0.6,
            "expected ~0.5s, got {}s",
            result
        );
    }

    //
    // Syncopated bass makes spurious fractional-BPM clusters that tie with the real one.

    #[test]
    fn detect_bpm_prefers_integer_bpm_over_fractional() {
        let sr = 44100u32;
        let true_bpm = 146.0f64;
        let beat_samples = (60.0 * sr as f64 / true_bpm).round() as usize;

        // Onsets every beat at 146 BPM. 80 Hz bursts survive the 150 Hz BPM lowpass.
        let n_onsets = 30;
        let burst_len = 2000usize;
        let total = beat_samples * n_onsets + burst_len;
        let mut signal = vec![0.0f32; total];
        for onset in 0..n_onsets {
            let start = onset * beat_samples;
            for j in 0..burst_len {
                signal[start + j] =
                    (2.0 * std::f32::consts::PI * 80.0 * j as f32 / sr as f32).sin();
            }
        }

        let detected = detect_bpm(&signal, sr, BPM_MIN, BPM_MAX).expect("should detect a BPM");
        assert!(
            (detected - true_bpm).abs() < 2.0,
            "expected ~{} BPM, got {} (fractional alias?)",
            true_bpm,
            detected
        );
        assert_eq!(
            detected,
            (detected * 10.0).round() / 10.0,
            "result should round to one decimal"
        );
    }
}
