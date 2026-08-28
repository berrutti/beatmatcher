pub fn read_wav_f32(path: &str) -> Result<(Vec<f32>, u32, u16), String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut f = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut b4 = [0u8; 4];

    macro_rules! r4 {
        () => {{
            f.read_exact(&mut b4).map_err(|e| e.to_string())?;
            b4
        }};
    }
    macro_rules! ru32 {
        () => {
            u32::from_le_bytes(r4!())
        };
    }
    macro_rules! ru16 {
        () => {{
            let mut b = [0u8; 2];
            f.read_exact(&mut b).map_err(|e| e.to_string())?;
            u16::from_le_bytes(b)
        }};
    }

    if &r4!() != b"RIFF" {
        return Err(format!("{path}: not a RIFF file"));
    }
    let _ = ru32!(); // file size
    if &r4!() != b"WAVE" {
        return Err(format!("{path}: not a WAVE file"));
    }

    let mut sample_rate = 0u32;
    let mut bit_depth = 0u16;
    let mut channels = 0u16;
    let mut data_offset = 0u64;
    let mut data_bytes = 0u32;

    loop {
        let mut id = [0u8; 4];
        if f.read_exact(&mut id).is_err() {
            break;
        }
        let chunk_size = ru32!();
        match &id {
            b"fmt " => {
                let _audio_fmt = ru16!();
                channels = ru16!();
                sample_rate = ru32!();
                let _ = ru32!(); // byte rate
                let _ = ru16!(); // block align
                bit_depth = ru16!();
                let extra = chunk_size.saturating_sub(16);
                if extra > 0 {
                    f.seek(SeekFrom::Current(extra as i64)).ok();
                }
            }
            b"data" => {
                data_bytes = chunk_size;
                data_offset = f.stream_position().map_err(|e| e.to_string())?;
                break;
            }
            _ => {
                f.seek(SeekFrom::Current(chunk_size as i64)).ok();
            }
        }
    }

    if data_offset == 0 {
        return Err(format!("{path}: no data chunk"));
    }

    let mut raw = vec![0u8; data_bytes as usize];
    f.seek(SeekFrom::Start(data_offset))
        .map_err(|e| e.to_string())?;
    f.read_exact(&mut raw).map_err(|e| e.to_string())?;

    let samples: Vec<f32> = match bit_depth {
        16 => raw
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
            .collect(),
        24 => raw
            .chunks_exact(3)
            .map(|b| {
                let v =
                    i32::from_le_bytes([b[0], b[1], b[2], if b[2] & 0x80 != 0 { 0xff } else { 0 }]);
                v as f32 / 8_388_608.0
            })
            .collect(),
        32 => raw
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect(),
        _ => return Err(format!("{path}: unsupported bit depth {bit_depth}")),
    };

    Ok((samples, sample_rate, channels))
}

pub fn write_wav_f32(
    path: &str,
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
) -> Result<(), String> {
    use std::io::Write;
    let mut f = std::fs::File::create(path).map_err(|e| format!("{path}: {e}"))?;
    let byte_count = (samples.len() * 4) as u32;
    f.write_all(b"RIFF").map_err(|e| e.to_string())?;
    f.write_all(&(36 + byte_count).to_le_bytes())
        .map_err(|e| e.to_string())?;
    f.write_all(b"WAVE").map_err(|e| e.to_string())?;
    f.write_all(b"fmt ").map_err(|e| e.to_string())?;
    f.write_all(&16u32.to_le_bytes())
        .map_err(|e| e.to_string())?;
    f.write_all(&3u16.to_le_bytes())
        .map_err(|e| e.to_string())?; // IEEE float
    f.write_all(&channels.to_le_bytes())
        .map_err(|e| e.to_string())?;
    f.write_all(&sample_rate.to_le_bytes())
        .map_err(|e| e.to_string())?;
    f.write_all(&(sample_rate * channels as u32 * 4).to_le_bytes())
        .map_err(|e| e.to_string())?;
    f.write_all(&(channels * 4).to_le_bytes())
        .map_err(|e| e.to_string())?;
    f.write_all(&32u16.to_le_bytes())
        .map_err(|e| e.to_string())?;
    f.write_all(b"data").map_err(|e| e.to_string())?;
    f.write_all(&byte_count.to_le_bytes())
        .map_err(|e| e.to_string())?;
    for &s in samples {
        f.write_all(&s.to_le_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn write_flac_f32(path: &str, samples: &[f32], sample_rate: u32) -> Result<(), String> {
    use flacenc::bitsink::ByteSink;
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    use std::io::Write;

    const MAX_24BIT: f32 = 8_388_607.0;
    let source = SliceSource {
        samples: samples
            .iter()
            .map(|&s| (s.clamp(-1.0, 1.0) * MAX_24BIT) as i32)
            .collect(),
        channels: 2,
        bits_per_sample: 24,
        sample_rate: sample_rate as usize,
        pos: 0,
    };

    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|e| format!("FLAC config error: {e:?}"))?;
    let block_size = config.block_size;
    let stream = flacenc::encode_with_fixed_block_size(&config, source, block_size)
        .map_err(|e| format!("FLAC encode error: {e:?}"))?;

    let mut sink = ByteSink::with_capacity(stream.count_bits());
    stream
        .write(&mut sink)
        .map_err(|e| format!("FLAC write error: {e:?}"))?;

    std::fs::File::create(path)
        .and_then(|mut f| f.write_all(sink.as_slice()))
        .map_err(|e| format!("{path}: {e}"))
}

struct SliceSource {
    samples: Vec<i32>,
    channels: usize,
    bits_per_sample: usize,
    sample_rate: usize,
    pos: usize,
}

impl flacenc::source::Source for SliceSource {
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
        Some(self.samples.len() / self.channels)
    }

    fn read_samples<F: flacenc::source::Fill>(
        &mut self,
        block_size: usize,
        dest: &mut F,
    ) -> Result<usize, flacenc::error::SourceError> {
        let n = (block_size * self.channels).min(self.samples.len() - self.pos);
        if n == 0 {
            return Ok(0);
        }
        dest.fill_interleaved(&self.samples[self.pos..self.pos + n])?;
        self.pos += n;
        Ok(n / self.channels)
    }
}
