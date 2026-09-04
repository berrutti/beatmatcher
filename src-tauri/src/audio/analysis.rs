use super::dsp::Biquad;
use super::io::DecodedPacket;

const BAND_BASS_HZ: f32 = 250.0;
const BAND_MID_HZ: f32 = 2_000.0;

// 6.7 ms a point, finer than a device pixel at every zoom the edit view offers, and the
// rate the established waveform formats use.
pub const DENSE_POINTS_PER_SEC: f64 = 150.0;

#[derive(Default)]
pub struct StreamedBands {
    pub bass: Vec<f32>,
    pub mid: Vec<f32>,
    pub high: Vec<f32>,
    pub bass_rms: f32,
    pub mid_rms: f32,
    pub high_rms: f32,
}

/// Fed from the decoder, so points reach `on_points` while the file is still being read.
/// `None` when no packet arrived.
pub fn reduce_packets(
    packets: std::sync::mpsc::Receiver<DecodedPacket>,
    points_per_chunk: usize,
    mut on_total_points: impl FnMut(usize),
    mut on_points: impl FnMut(&[f32], usize),
) -> Option<StreamedBands> {
    let mut stream: Option<BandStream> = None;
    while let Ok((packet, shape)) = packets.recv() {
        let reducer = stream.get_or_insert_with(|| {
            let started = BandStream::new(
                shape.total_frames,
                shape.channels,
                shape.sample_rate,
                points_per_chunk,
            );
            on_total_points(started.total_points());
            started
        });
        reducer.push(&packet, &mut on_points);
    }
    // Closed here rather than by the caller: the last points would otherwise wait for the
    // load to finish before they could be painted.
    stream.map(|stream| stream.finish(&mut on_points))
}

pub fn dense_point_count(total_frames: usize, sample_rate: u32) -> usize {
    if sample_rate == 0 || total_frames == 0 {
        return 0;
    }
    let seconds = total_frames as f64 / sample_rate as f64;
    (seconds * DENSE_POINTS_PER_SEC).ceil() as usize
}

/// The reduction as a resumable object, so it can be driven from the decoder while the
/// file is still being read instead of waiting for the whole buffer.
pub struct BandStream {
    channels: usize,
    total_frames: usize,
    total_points: usize,
    frames_per_point: f64,
    lp_bass: Biquad,
    lp_bass_mid: Biquad,
    bass: Vec<f32>,
    mid: Vec<f32>,
    high: Vec<f32>,
    sum_bass_sq: f64,
    sum_mid_sq: f64,
    sum_high_sq: f64,
    frame: usize,
    points_done: usize,
    bin_bass: f32,
    bin_mid: f32,
    bin_high: f32,
    bin_amp: f32,
    bin_count: f32,
    chunk: Vec<f32>,
    points_per_chunk: usize,
}

impl BandStream {
    pub fn new(
        total_frames: usize,
        channels: usize,
        sample_rate: u32,
        points_per_chunk: usize,
    ) -> Self {
        let sr = sample_rate as f32;
        let butterworth_q = 1.0 / std::f32::consts::SQRT_2;
        let total_points = dense_point_count(total_frames, sample_rate);
        Self {
            channels,
            total_frames,
            total_points,
            frames_per_point: if total_points == 0 {
                0.0
            } else {
                total_frames as f64 / total_points as f64
            },
            lp_bass: Biquad::low_pass(sr, BAND_BASS_HZ, butterworth_q),
            lp_bass_mid: Biquad::low_pass(sr, BAND_MID_HZ, butterworth_q),
            bass: Vec::with_capacity(total_frames),
            mid: Vec::with_capacity(total_frames),
            high: Vec::with_capacity(total_frames),
            sum_bass_sq: 0.0,
            sum_mid_sq: 0.0,
            sum_high_sq: 0.0,
            frame: 0,
            points_done: 0,
            bin_bass: 0.0,
            bin_mid: 0.0,
            bin_high: 0.0,
            bin_amp: 0.0,
            bin_count: 0.0,
            chunk: Vec::with_capacity(points_per_chunk * 4),
            points_per_chunk,
        }
    }

    pub fn total_points(&self) -> usize {
        self.total_points
    }

    pub fn push(&mut self, samples: &[f32], mut on_points: impl FnMut(&[f32], usize)) {
        if self.channels == 0 || self.total_points == 0 {
            return;
        }
        let frames = samples.len() / self.channels;
        for local in 0..frames {
            if self.frame >= self.total_frames {
                break;
            }
            let base = local * self.channels;
            let mono: f32 =
                samples[base..base + self.channels].iter().sum::<f32>() / self.channels as f32;
            let b = self.lp_bass.process(mono);
            let bm = self.lp_bass_mid.process(mono);
            let m = bm - b;
            let h = mono - bm;
            self.bass.push(b);
            self.mid.push(m);
            self.high.push(h);
            self.sum_bass_sq += (b as f64) * (b as f64);
            self.sum_mid_sq += (m as f64) * (m as f64);
            self.sum_high_sq += (h as f64) * (h as f64);
            self.bin_bass += b * b;
            self.bin_mid += m * m;
            self.bin_high += h * h;
            for ch in 0..self.channels {
                let sample = samples[base + ch];
                self.bin_amp += sample * sample;
            }
            self.bin_count += 1.0;
            self.frame += 1;

            let point_end = (((self.points_done + 1) as f64 * self.frames_per_point) as usize)
                .min(self.total_frames);
            if self.frame >= point_end && self.points_done < self.total_points {
                self.close_point();
                if self.chunk.len() >= self.points_per_chunk * 4 {
                    on_points(&self.chunk, self.total_points);
                    self.chunk.clear();
                }
            }
        }
    }

    fn close_point(&mut self) {
        if self.bin_count > 0.0 {
            self.chunk.push((self.bin_bass / self.bin_count).sqrt());
            self.chunk.push((self.bin_mid / self.bin_count).sqrt());
            self.chunk.push((self.bin_high / self.bin_count).sqrt());
            self.chunk.push(
                (self.bin_amp / (self.bin_count * self.channels as f32))
                    .sqrt()
                    .min(1.0),
            );
        } else {
            self.chunk.extend_from_slice(&[0.0, 0.0, 0.0, 0.0]);
        }
        self.bin_bass = 0.0;
        self.bin_mid = 0.0;
        self.bin_high = 0.0;
        self.bin_amp = 0.0;
        self.bin_count = 0.0;
        self.points_done += 1;
    }

    pub fn finish(mut self, mut on_points: impl FnMut(&[f32], usize)) -> StreamedBands {
        while self.points_done < self.total_points {
            self.close_point();
        }
        if !self.chunk.is_empty() {
            on_points(&self.chunk, self.total_points);
            self.chunk.clear();
        }
        // What arrived, not what the container declared: n_frames is approximate in some
        // containers, and dividing by it would scale every band level by the error.
        let frames = self.frame.max(1) as f64;
        StreamedBands {
            bass: self.bass,
            mid: self.mid,
            high: self.high,
            bass_rms: (self.sum_bass_sq / frames).sqrt() as f32,
            mid_rms: (self.sum_mid_sq / frames).sqrt() as f32,
            high_rms: (self.sum_high_sq / frames).sqrt() as f32,
        }
    }
}

/// The three bands sum to the mono signal, so their levels in quadrature stand in for the
/// track's own.
pub fn band_reference(bands: &super::SpectralBands) -> f32 {
    let total = (bands.bass_rms * bands.bass_rms
        + bands.mid_rms * bands.mid_rms
        + bands.high_rms * bands.high_rms)
        .sqrt();
    if total > 0.0 {
        total
    } else {
        1.0
    }
}

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
    // One reference for all three bands, so a bin's values keep the loudness ratio between
    // them. Dividing each band by its own level instead makes every band average 1, which
    // leaves the three stacked heights the same and the display saying nothing.
    let reference = band_reference(bands);
    if bass.is_empty() || num_points == 0 {
        return vec![0.0; num_points * 4];
    }
    let band_rate = if bands.source_rate > 0 {
        bands.source_rate
    } else {
        sample_rate
    } as f64;
    let total_frames = bass.len();
    let start_frame = (start_sec * band_rate).max(0.0) as usize;
    let end_frame = ((end_sec * band_rate) as usize).min(total_frames);

    if start_frame >= end_frame {
        return vec![0.0; num_points * 4];
    }

    let visible_frames = end_frame - start_frame;
    let frames_per_point = visible_frames as f64 / num_points as f64;
    // The deck's own rate, which the resampler may have moved away from the file's.
    let sample_frames = samples.len() / channels.max(1);
    let sample_scale = sample_rate as f64 / band_rate;

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

        let mut sample_count = 0.0f32;
        for frame in bin_start..bin_end {
            sum_bass_sq += bass[frame] * bass[frame];
            sum_mid_sq += mid[frame] * mid[frame];
            sum_high_sq += high[frame] * high[frame];
            let at = (frame as f64 * sample_scale) as usize;
            if at < sample_frames {
                for ch in 0..channels {
                    let s = samples[at * channels + ch];
                    sum_sample_sq += s * s;
                }
                sample_count += 1.0;
            }
        }

        let rms_amp = if sample_count > 0.0 {
            (sum_sample_sq / (sample_count * channels as f32)).sqrt()
        } else {
            0.0
        };
        let rms_bass = (sum_bass_sq / count).sqrt();
        let rms_mid = (sum_mid_sq / count).sqrt();
        let rms_high = (sum_high_sq / count).sqrt();

        // Unclamped: the frontend takes only the ratio between the three. The amplitude
        // is stored raw so the sqrt curve in the frontend has range left to spread.
        let r = rms_bass / reference;
        let g = rms_mid / reference;
        let b = rms_high / reference;
        let amp = rms_amp.min(1.0);

        result.push(r);
        result.push(g);
        result.push(b);
        result.push(amp);
    }

    result
}

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
// between 60-150 Hz. Cutting above 150 Hz removes mid/snare content that
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

        assert!(
            result[2] > 0.5,
            "expected loud bin at index 2, got {:?}",
            result
        );
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

    fn tone(frames: usize, channels: usize) -> Vec<f32> {
        (0..frames * channels)
            .map(|i| {
                let t = (i / channels) as f32 / 44100.0;
                0.6 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
                    + 0.2 * (2.0 * std::f32::consts::PI * 5000.0 * t).sin()
            })
            .collect()
    }

    fn points_from(samples: &[f32], channels: usize, chunk_frames: usize) -> Vec<f32> {
        let frames = samples.len() / channels;
        let mut stream = BandStream::new(frames, channels, 44100, 64);
        let mut points = Vec::new();
        let mut at = 0;
        while at < frames {
            let end = (at + chunk_frames).min(frames);
            stream.push(&samples[at * channels..end * channels], |chunk, _| {
                points.extend_from_slice(chunk)
            });
            at = end;
        }
        stream.finish(|chunk, _| points.extend_from_slice(chunk));
        points
    }

    #[test]
    fn the_chunk_size_the_decoder_hands_over_does_not_change_the_points() {
        let samples = tone(44100, 2);
        let whole = points_from(&samples, 2, 44100);
        for chunk in [1, 7, 512, 4096] {
            assert_eq!(points_from(&samples, 2, chunk), whole, "chunk of {chunk}");
        }
    }

    #[test]
    fn every_declared_point_is_emitted_once() {
        let samples = tone(44100, 2);
        let stream = BandStream::new(44100, 2, 44100, 64);
        let expected = stream.total_points();
        assert_eq!(points_from(&samples, 2, 512).len(), expected * 4);
    }

    #[test]
    fn a_band_level_counts_only_the_frames_that_arrived() {
        let samples = tone(44100, 2);
        // The declared length is what the container claimed; only half of it turns up.
        let half = samples.len() / 2;
        let mut short = BandStream::new(44100, 2, 44100, 64);
        short.push(&samples[..half], |_, _| {});
        let short = short.finish(|_, _| {});

        let mut exact = BandStream::new(22050, 2, 44100, 64);
        exact.push(&samples[..half], |_, _| {});
        let exact = exact.finish(|_, _| {});

        assert!((short.bass_rms - exact.bass_rms).abs() < 1e-6);
        assert!((short.high_rms - exact.high_rms).abs() < 1e-6);
    }

    #[test]
    fn packets_reduce_to_the_same_points_as_one_push() {
        let samples = tone(44100, 2);
        let (send, packets) = std::sync::mpsc::channel();
        for chunk in samples.chunks(512 * 2) {
            send.send((
                std::sync::Arc::new(chunk.to_vec()),
                super::super::io::DecodedShape {
                    channels: 2,
                    total_frames: 44100,
                    sample_rate: 44100,
                },
            ))
            .ok();
        }
        drop(send);

        let mut declared = None;
        let mut points = Vec::new();
        let bands = reduce_packets(
            packets,
            64,
            |total| declared = Some(total),
            |chunk, _| points.extend_from_slice(chunk),
        )
        .expect("packets arrived");

        assert_eq!(declared, Some(dense_point_count(44100, 44100)));
        assert_eq!(points, points_from(&samples, 2, 512));
        assert!(bands.bass_rms > 0.0);
    }

    #[test]
    fn no_packet_reduces_to_no_bands() {
        let (send, packets) = std::sync::mpsc::channel();
        drop(send);
        let mut declared = false;
        assert!(reduce_packets(packets, 64, |_| declared = true, |_, _| {}).is_none());
        assert!(!declared);
    }

    #[test]
    fn dense_point_count_follows_the_rate() {
        assert_eq!(
            dense_point_count(44100, 44100),
            DENSE_POINTS_PER_SEC as usize
        );
        assert_eq!(dense_point_count(0, 44100), 0);
        assert_eq!(dense_point_count(44100, 0), 0);
    }

    #[test]
    fn the_band_reference_is_never_zero() {
        let silent = super::super::SpectralBands::default();
        assert!(band_reference(&silent) > 0.0);
    }

    #[test]
    fn band_reference_matches_the_typescript_fixture() {
        let bands = super::super::SpectralBands {
            bass_rms: 0.4,
            mid_rms: 0.2,
            high_rms: 0.1,
            ..Default::default()
        };
        // f64, so the literals carry the digits the TypeScript side compares against.
        let reference = f64::from(band_reference(&bands));
        assert!((reference - 0.45825757).abs() < 1e-6);
        assert!((reference / 0.4 - 1.14564392).abs() < 1e-6);
        assert!((reference / 0.2 - 2.29128785).abs() < 1e-6);
        assert!((reference / 0.1 - 4.58257569).abs() < 1e-6);
    }

    // Bands are reduced from the decoder at the file's own rate, while the deck holds
    // samples resampled for the device, so a region has to walk the two at their own rates.
    #[test]
    fn a_region_reads_bands_and_samples_at_their_own_rates() {
        let native_rate = 44100;
        let device_rate = 48000;
        let seconds = 2.0;
        let native_frames = (native_rate as f64 * seconds) as usize;
        let device_frames = (device_rate as f64 * seconds) as usize;

        let tone_at = |frames: usize, rate: u32| -> Vec<f32> {
            (0..frames * 2)
                .map(|i| {
                    let t = (i / 2) as f32 / rate as f32;
                    0.5 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
                })
                .collect()
        };

        let native = tone_at(native_frames, native_rate);
        let device = tone_at(device_frames, device_rate);

        let mut stream = BandStream::new(native_frames, 2, native_rate, 64);
        stream.push(&native, |_, _| {});
        let reduced = stream.finish(|_, _| {});

        let bands = super::super::SpectralBands {
            bass: std::sync::Arc::new(reduced.bass),
            mid: std::sync::Arc::new(reduced.mid),
            high: std::sync::Arc::new(reduced.high),
            bass_rms: reduced.bass_rms,
            mid_rms: reduced.mid_rms,
            high_rms: reduced.high_rms,
            source_rate: native_rate,
        };

        let points =
            compute_spectral_waveform_region(&device, 2, &bands, device_rate, 0.0, seconds, 8);
        assert_eq!(points.len(), 32);
        // A steady tone: every point should carry the same bass share and amplitude.
        for point in 1..8 {
            assert!(
                (points[point * 4] - points[0]).abs() < 0.05,
                "bass share drifted at point {point}: {} vs {}",
                points[point * 4],
                points[0]
            );
            assert!(
                (points[point * 4 + 3] - points[3]).abs() < 0.05,
                "amplitude drifted at point {point}"
            );
        }
    }
}
