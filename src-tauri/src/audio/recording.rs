pub(crate) struct RecordingState {
    pub(crate) thread: std::thread::JoinHandle<Result<(), String>>,
    pub(crate) temp_path: String,
}

// Dedicated writer thread; on channel close it back-patches the RIFF/data sizes.
pub(crate) fn wav_writer_thread(
    path: String,
    sample_rate: u32,
    bit_depth: u16,
    receiver: std::sync::mpsc::Receiver<Vec<f32>>,
) -> Result<(), String> {
    use std::io::{Seek, SeekFrom, Write};

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
    }

    buf.flush().map_err(|error| error.to_string())?;

    let riff_size = data_bytes.saturating_add(36);
    let mut file = buf.into_inner().map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(4))
        .map_err(|error| error.to_string())?;
    file.write_all(&riff_size.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(40))
        .map_err(|error| error.to_string())?;
    file.write_all(&data_bytes.to_le_bytes())
        .map_err(|error| error.to_string())?;

    Ok(())
}

// Streams f32 samples from the recording channel to a temp .pcm file as i32 LE
// (converting on the fly), then encodes that file to FLAC block-by-block via a
// custom Source impl. Peak RAM during recording: O(one chunk). During encode:
// O(one FLAC block, ~32 KB). 32-bit source → 24-bit FLAC; 16-bit → 16-bit FLAC.
pub(crate) fn flac_writer_thread(
    path: String,
    sample_rate: u32,
    bit_depth: u16,
    receiver: std::sync::mpsc::Receiver<Vec<f32>>,
) -> Result<(), String> {
    use std::io::Write;

    let pcm_path = format!("{}.pcm", path);
    const MAX_24BIT: f32 = 8_388_607.0; // 2^23 - 1
    const MAX_16BIT: f32 = 32_767.0; // 2^15 - 1
    let (scale, flac_bits): (f32, usize) = if bit_depth == 32 {
        (MAX_24BIT, 24)
    } else {
        (MAX_16BIT, 16)
    };

    let mut total_samples_per_channel: usize = 0;
    {
        let file = std::fs::File::create(&pcm_path).map_err(|error| error.to_string())?;
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
    }

    let source = PcmFileSource::open(
        &pcm_path,
        2,
        flac_bits,
        sample_rate as usize,
        total_samples_per_channel,
    )
    .map_err(|error| error.to_string())?;

    use flacenc::bitsink::ByteSink;
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;

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

    let mut out = std::fs::File::create(&path).map_err(|error| error.to_string())?;
    out.write_all(sink.as_slice())
        .map_err(|error| error.to_string())?;

    std::fs::remove_file(&pcm_path).ok();
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
}

impl PcmFileSource {
    fn open(
        path: &str,
        channels: usize,
        bits_per_sample: usize,
        sample_rate: usize,
        total_samples_per_channel: usize,
    ) -> std::io::Result<Self> {
        let file = std::fs::File::open(path)?;
        Ok(Self {
            reader: std::io::BufReader::new(file),
            channels,
            bits_per_sample,
            sample_rate,
            total_samples_per_channel,
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
        Ok(per_channel)
    }
}
