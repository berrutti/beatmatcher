pub(crate) struct Recording {
    pub(crate) thread: std::thread::JoinHandle<Result<(), String>>,
    pub(crate) temp_path: String,
}

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

pub(crate) fn wav_writer_thread(
    path: String,
    sample_rate: u32,
    bit_depth: u16,
    receiver: std::sync::mpsc::Receiver<Vec<f32>>,
) -> Result<(), String> {
    use std::io::Write;

    let file = std::fs::File::create(&path).map_err(|error| error.to_string())?;
    let mut buf = std::io::BufWriter::new(file);

    let channels: u16 = 2;
    let bytes_per_sample: u32 = (bit_depth as u32) / 8;
    let byte_rate = sample_rate * channels as u32 * bytes_per_sample;
    let block_align = (channels as u32 * bytes_per_sample) as u16;
    let format_tag: u16 = if bit_depth == 32 { 3 } else { 1 }; // 3=IEEE float, 1=PCM

    buf.write_all(b"RIFF").map_err(|error| error.to_string())?;
    buf.write_all(&0u32.to_le_bytes())
        .map_err(|error| error.to_string())?;
    buf.write_all(b"WAVE").map_err(|error| error.to_string())?;
    buf.write_all(b"fmt ").map_err(|error| error.to_string())?;
    buf.write_all(&16u32.to_le_bytes())
        .map_err(|error| error.to_string())?;
    buf.write_all(&format_tag.to_le_bytes())
        .map_err(|error| error.to_string())?;
    buf.write_all(&channels.to_le_bytes())
        .map_err(|error| error.to_string())?;
    buf.write_all(&sample_rate.to_le_bytes())
        .map_err(|error| error.to_string())?;
    buf.write_all(&byte_rate.to_le_bytes())
        .map_err(|error| error.to_string())?;
    buf.write_all(&block_align.to_le_bytes())
        .map_err(|error| error.to_string())?;
    buf.write_all(&bit_depth.to_le_bytes())
        .map_err(|error| error.to_string())?;
    buf.write_all(b"data").map_err(|error| error.to_string())?;
    buf.write_all(&0u32.to_le_bytes())
        .map_err(|error| error.to_string())?;

    let mut data_bytes = 0u32;
    let mut synced_at = 0u32;

    while let Ok(chunk) = receiver.recv() {
        for &sample in &chunk {
            if bit_depth == 16 {
                let quantized = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
                buf.write_all(&quantized.to_le_bytes())
                    .map_err(|error| error.to_string())?;
                data_bytes = data_bytes.saturating_add(2);
            } else {
                buf.write_all(&sample.to_le_bytes())
                    .map_err(|error| error.to_string())?;
                data_bytes = data_bytes.saturating_add(4);
            }
        }
        if data_bytes.saturating_sub(synced_at) >= SIZE_SYNC_BYTES {
            synced_at = data_bytes;
            write_sizes(&mut buf, data_bytes)?;
        }
    }

    write_sizes(&mut buf, data_bytes)?;
    Ok(())
}

/// About three seconds of stereo float at 48 kHz. A recording killed between syncs
/// still opens: only the samples past the last one are outside the declared size.
const SIZE_SYNC_BYTES: u32 = 1_000_000;

/// RIFF declares its lengths in the header, so a file whose sizes are only written at
/// the end reads as empty when the process dies. Rewritten as the recording grows.
fn write_sizes(buf: &mut std::io::BufWriter<std::fs::File>, data_bytes: u32) -> Result<(), String> {
    use std::io::{Seek, SeekFrom, Write};

    buf.flush().map_err(|error| error.to_string())?;
    let file = buf.get_mut();
    let end = file.stream_position().map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(4))
        .map_err(|error| error.to_string())?;
    file.write_all(&data_bytes.saturating_add(36).to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(40))
        .map_err(|error| error.to_string())?;
    file.write_all(&data_bytes.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(end))
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) const SAVE_DONE: u32 = 1000;

/// Distinct from any permille, so "nothing is saving" needs no second flag.
pub(crate) const SAVE_IDLE: u32 = u32::MAX;

/// Held below `SAVE_DONE`, which belongs to the store that ends the save: a dial
/// reading complete while work remains is worse than one reading 999.
pub(crate) fn save_permille(done: u64, total: u64) -> Option<u32> {
    let permille = done
        .saturating_mul(u64::from(SAVE_DONE))
        .checked_div(total)?;
    Some(
        u32::try_from(permille)
            .unwrap_or(SAVE_DONE)
            .min(SAVE_DONE - 1),
    )
}

// Via a temp .pcm file rather than buffering the take: peak RAM is one chunk
// while recording and one FLAC block while encoding, whatever the length.
pub(crate) fn flac_writer_thread(
    path: String,
    sample_rate: u32,
    bit_depth: u16,
    receiver: std::sync::mpsc::Receiver<Vec<f32>>,
    progress: Arc<AtomicU32>,
) -> Result<(), String> {
    let pcm_path = format!("{}.pcm", path);
    const MAX_24BIT: f32 = 8_388_607.0; // 2^23 - 1
    const MAX_16BIT: f32 = 32_767.0; // 2^15 - 1
    let (scale, flac_bits): (f32, usize) = if bit_depth == 32 {
        (MAX_24BIT, 24)
    } else {
        (MAX_16BIT, 16)
    };

    let total_samples_per_channel = drain_to_pcm(&pcm_path, scale, receiver)?;

    // Armed here rather than at the start of the take: nothing is saving while
    // the channel is still being drained.
    progress.store(0, Ordering::Relaxed);
    let encoded = encode_pcm_to_flac(
        &pcm_path,
        &path,
        flac_bits,
        sample_rate,
        total_samples_per_channel,
        &progress,
    );
    progress.store(SAVE_IDLE, Ordering::Relaxed);
    encoded?;

    std::fs::remove_file(&pcm_path).ok();
    Ok(())
}

fn drain_to_pcm(
    pcm_path: &str,
    scale: f32,
    receiver: std::sync::mpsc::Receiver<Vec<f32>>,
) -> Result<usize, String> {
    use std::io::Write;

    let mut total_samples_per_channel: usize = 0;
    let file = std::fs::File::create(pcm_path).map_err(|error| error.to_string())?;
    let mut buf = std::io::BufWriter::new(file);
    while let Ok(chunk) = receiver.recv() {
        for &sample in &chunk {
            let quantized = (sample.clamp(-1.0, 1.0) * scale) as i32;
            buf.write_all(&quantized.to_le_bytes())
                .map_err(|error| error.to_string())?;
        }
        total_samples_per_channel += chunk.len() / 2;
    }
    buf.flush().map_err(|error| error.to_string())?;
    Ok(total_samples_per_channel)
}

fn encode_pcm_to_flac(
    pcm_path: &str,
    path: &str,
    flac_bits: usize,
    sample_rate: u32,
    total_samples_per_channel: usize,
    progress: &Arc<AtomicU32>,
) -> Result<(), String> {
    use flacenc::bitsink::ByteSink;
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    use std::io::Write;

    let source = PcmFileSource::open(
        pcm_path,
        2,
        flac_bits,
        sample_rate as usize,
        total_samples_per_channel,
        Arc::clone(progress),
    )
    .map_err(|error| error.to_string())?;

    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|error| format!("FLAC config error: {:?}", error))?;
    let block_size = config.block_size;
    let stream = flacenc::encode_with_fixed_block_size(&config, source, block_size)
        .map_err(|error| format!("FLAC encode error: {:?}", error))?;

    let mut sink = ByteSink::with_capacity(stream.count_bits());
    stream
        .write(&mut sink)
        .map_err(|error| format!("FLAC write error: {:?}", error))?;

    let mut out = std::fs::File::create(path).map_err(|error| error.to_string())?;
    out.write_all(sink.as_slice())
        .map_err(|error| error.to_string())?;
    Ok(())
}

// flacenc's Source trait requires integer samples, so we pre-convert f32 to i32
// during the streaming phase rather than holding floats in memory.
struct PcmFileSource {
    reader: std::io::BufReader<std::fs::File>,
    channels: usize,
    bits_per_sample: usize,
    sample_rate: usize,
    total_samples_per_channel: usize,
    read_so_far: usize,
    progress: Arc<AtomicU32>,
}

impl PcmFileSource {
    fn open(
        path: &str,
        channels: usize,
        bits_per_sample: usize,
        sample_rate: usize,
        total_samples_per_channel: usize,
        progress: Arc<AtomicU32>,
    ) -> std::io::Result<Self> {
        let file = std::fs::File::open(path)?;
        Ok(Self {
            reader: std::io::BufReader::new(file),
            channels,
            bits_per_sample,
            sample_rate,
            total_samples_per_channel,
            read_so_far: 0,
            progress,
        })
    }
}

impl flacenc::source::Source for PcmFileSource {
    fn channels(&self) -> usize {
        self.channels
    }
    fn bits_per_sample(&self) -> usize {
        self.bits_per_sample
    }
    fn sample_rate(&self) -> usize {
        self.sample_rate
    }
    fn len_hint(&self) -> Option<usize> {
        Some(self.total_samples_per_channel)
    }

    fn read_samples<F: flacenc::source::Fill>(
        &mut self,
        block_size: usize,
        dest: &mut F,
    ) -> Result<usize, flacenc::error::SourceError> {
        use std::io::Read;
        let n_frames = block_size * self.channels;
        let mut bytes = vec![0u8; n_frames * 4];

        let mut total = 0;
        while total < bytes.len() {
            match self.reader.read(&mut bytes[total..]) {
                Ok(0) => break,
                Ok(read_bytes) => total += read_bytes,
                Err(_) => break,
            }
        }
        if total == 0 {
            return Ok(0);
        }

        let complete = (total / (self.channels * 4)) * (self.channels * 4);
        let per_channel = complete / (self.channels * 4);

        let samples: Vec<i32> = bytes[..complete]
            .chunks_exact(4)
            .map(|sample_bytes| {
                i32::from_le_bytes([
                    sample_bytes[0],
                    sample_bytes[1],
                    sample_bytes[2],
                    sample_bytes[3],
                ])
            })
            .collect();

        dest.fill_interleaved(&samples)?;
        // The encoder pulls from here, so draining the source is the only place
        // that knows how far a `encode_with_fixed_block_size` call has got.
        self.read_so_far += per_channel;
        let done = self.read_so_far.min(self.total_samples_per_channel);
        if let Some(permille) = save_permille(done as u64, self.total_samples_per_channel as u64) {
            self.progress.store(permille, Ordering::Relaxed);
        }
        Ok(per_channel)
    }
}
