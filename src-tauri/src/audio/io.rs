use serde::Serialize;

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrackTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub composer: Option<String>,
    pub remixer: Option<String>,
    pub label: Option<String>,
    pub comment: Option<String>,
    pub track_number: Option<String>,
    pub year: Option<String>,
    pub rating: Option<String>,
}

fn open_format(
    path: &str,
) -> Result<symphonia::core::probe::ProbeResult, Box<dyn std::error::Error + Send + Sync>> {
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        hint.with_extension(ext);
    }
    Ok(symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?)
}

/// What the caller would otherwise have to reopen the file to learn.
#[derive(Clone, Copy)]
pub struct DecodedShape {
    pub channels: usize,
    pub total_frames: usize,
    pub sample_rate: u32,
}

pub fn decode_audio(
    path: &str,
) -> Result<(Vec<f32>, usize, u32), Box<dyn std::error::Error + Send + Sync>> {
    decode_audio_streaming(path, |_, _| {})
}

/// `on_decoded` sees each run of frames as it is decoded, in order, so a caller can
/// analyse the head of a track while the tail is still being read. It also carries the
/// track's shape, which the caller cannot know without opening the file itself.
pub fn decode_audio_streaming(
    path: &str,
    mut on_decoded: impl FnMut(&[f32], DecodedShape),
) -> Result<(Vec<f32>, usize, u32), Box<dyn std::error::Error + Send + Sync>> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};

    let probed = open_format(path)?;

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("no audio track found")?;

    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    let sample_rate = codec_params.sample_rate.unwrap_or(44100);

    let mut decoder =
        symphonia::default::get_codecs().make(&codec_params, &DecoderOptions::default())?;

    let expected_frames = codec_params.n_frames.unwrap_or(0) as usize;
    let capacity = codec_params
        .n_frames
        .map(|frame_count| frame_count as usize * 2)
        .unwrap_or(0);
    let mut samples: Vec<f32> = Vec::with_capacity(capacity);
    // Determined from the first decoded packet spec rather than codec_params,
    // because codec_params.channels can be None for some formats even when the
    // audio is mono, which would cause wrong chunk-size when mixing to mono.
    let mut actual_channels: Option<usize> = None;

    log::info!(
        "decode_audio: opening '{}', native_sr={}",
        path,
        sample_rate
    );

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(_)) => break,
            Err(symphonia::core::errors::Error::ResetRequired) => continue,
            Err(error) => return Err(error.into()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                if decoded.frames() == 0 {
                    continue;
                }
                let spec = *decoded.spec();
                let src_channels = spec.channels.count();
                // Lock in the channel count from the first packet.
                let out_channels = *actual_channels.get_or_insert_with(|| {
                    let channel_count = src_channels.min(2);
                    log::info!(
                        "decode_audio: first packet spec src_channels={} out_channels={}",
                        src_channels,
                        channel_count
                    );
                    channel_count
                });
                let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buf.copy_interleaved_ref(decoded);
                let src = buf.samples();
                let appended_from = samples.len();
                if src_channels <= 2 {
                    samples.extend_from_slice(src);
                } else {
                    // More than stereo: keep only L and R
                    for frame in src.chunks(src_channels) {
                        samples.push(frame[0]);
                        if out_channels > 1 {
                            samples.push(frame[1]);
                        }
                    }
                }
                on_decoded(
                    &samples[appended_from..],
                    DecodedShape {
                        channels: out_channels,
                        total_frames: expected_frames,
                        sample_rate,
                    },
                );
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(error) => return Err(error.into()),
        }
    }

    let channels = actual_channels.unwrap_or(1);
    let total_frames = samples.len() / channels.max(1);
    log::info!(
        "decode_audio: done, channels={}, frames={}, duration={:.2}s",
        channels,
        total_frames,
        total_frames as f64 / sample_rate as f64
    );

    Ok((samples, channels, sample_rate))
}

pub fn resample_linear(input: &[f32], in_channels: usize, in_rate: u32, out_rate: u32) -> Vec<f32> {
    let in_frames = input.len() / in_channels;
    let ratio = in_rate as f64 / out_rate as f64;
    let out_frames = (in_frames as f64 / ratio).ceil() as usize;
    let mut output = Vec::with_capacity(out_frames * in_channels);

    for out_frame in 0..out_frames {
        let src_pos = out_frame as f64 * ratio;
        let src_frame = src_pos as usize;
        let interp_factor = (src_pos - src_frame as f64) as f32;
        let lo_frame = src_frame.min(in_frames.saturating_sub(1));
        let hi_frame = (src_frame + 1).min(in_frames.saturating_sub(1));

        for channel in 0..in_channels {
            let lo_sample = input[lo_frame * in_channels + channel];
            let hi_sample = input[hi_frame * in_channels + channel];
            output.push(lo_sample + interp_factor * (hi_sample - lo_sample));
        }
    }

    output
}

fn fill_tags_from_slice(tags: &[symphonia::core::meta::Tag], out: &mut TrackTags) {
    use symphonia::core::meta::StandardTagKey;
    for tag in tags {
        let value = || tag.value.to_string();
        match tag.std_key {
            Some(StandardTagKey::TrackTitle) if out.title.is_none() => out.title = Some(value()),
            Some(StandardTagKey::Artist) if out.artist.is_none() => out.artist = Some(value()),
            Some(StandardTagKey::Album) if out.album.is_none() => out.album = Some(value()),
            Some(StandardTagKey::AlbumArtist) if out.album_artist.is_none() => {
                out.album_artist = Some(value())
            }
            Some(StandardTagKey::Genre) if out.genre.is_none() => out.genre = Some(value()),
            Some(StandardTagKey::Composer) if out.composer.is_none() => {
                out.composer = Some(value())
            }
            Some(StandardTagKey::Remixer) if out.remixer.is_none() => out.remixer = Some(value()),
            Some(StandardTagKey::Label) if out.label.is_none() => out.label = Some(value()),
            Some(StandardTagKey::Comment) if out.comment.is_none() => out.comment = Some(value()),
            Some(StandardTagKey::TrackNumber) if out.track_number.is_none() => {
                out.track_number = Some(value())
            }
            // Prefer the plain release date/year. Fall back to the original
            // release date if that's all the file has.
            Some(StandardTagKey::Date) if out.year.is_none() => out.year = Some(value()),
            Some(StandardTagKey::ReleaseDate) if out.year.is_none() => out.year = Some(value()),
            Some(StandardTagKey::OriginalDate) if out.year.is_none() => out.year = Some(value()),
            Some(StandardTagKey::Rating) if out.rating.is_none() => out.rating = Some(value()),
            _ => {}
        }
    }
}

pub fn read_tags(path: &str) -> TrackTags {
    let mut probed = match open_format(path) {
        Ok(probe_result) => probe_result,
        Err(_) => return TrackTags::default(),
    };

    let mut tags = TrackTags::default();

    // Tags embedded before the format container (ID3v2 in MP3, APEv2, etc.)
    if let Some(rev) = probed
        .metadata
        .get()
        .and_then(|metadata| metadata.current().cloned())
    {
        fill_tags_from_slice(rev.tags(), &mut tags);
    }

    // Tags from the format reader itself (FLAC Vorbis comments, M4A atoms, etc.)
    let mut format = probed.format;
    if let Some(rev) = format.metadata().current() {
        fill_tags_from_slice(rev.tags(), &mut tags);
    }

    tags
}

pub fn read_cover_art(path: &str) -> Option<String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use symphonia::core::meta::StandardVisualKey;

    fn first_visual(visuals: &[symphonia::core::meta::Visual]) -> Option<String> {
        let visual = visuals
            .iter()
            .find(|visual| visual.usage == Some(StandardVisualKey::FrontCover))
            .or_else(|| visuals.first())?;
        let media_type = if visual.media_type.is_empty() {
            "image/jpeg"
        } else {
            &visual.media_type
        };
        Some(format!(
            "data:{};base64,{}",
            media_type,
            STANDARD.encode(&*visual.data)
        ))
    }

    let mut probed = open_format(path).ok()?;

    if let Some(rev) = probed
        .metadata
        .get()
        .and_then(|metadata| metadata.current().cloned())
    {
        if let Some(url) = first_visual(rev.visuals()) {
            return Some(url);
        }
    }

    let mut format = probed.format;
    if let Some(rev) = format.metadata().current() {
        if let Some(url) = first_visual(rev.visuals()) {
            return Some(url);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_linear_identity_passes_through() {
        let input = vec![0.0f32, 0.25, 0.5, 0.75, 1.0];
        let output = resample_linear(&input, 1, 44100, 44100);
        assert_eq!(output.len(), input.len());
        for (input_sample, output_sample) in input.iter().zip(output.iter()) {
            assert!(
                (input_sample - output_sample).abs() < 1e-6,
                "input={} output={}",
                input_sample,
                output_sample
            );
        }
    }

    #[test]
    fn resample_linear_downsample_halves_length() {
        let input: Vec<f32> = (0..100).map(|index| index as f32 / 100.0).collect();
        let output = resample_linear(&input, 1, 44100, 22050);
        assert!(
            output.len() >= 49 && output.len() <= 51,
            "expected ~50 frames, got {}",
            output.len()
        );
    }

    #[test]
    fn resample_linear_upsample_doubles_length() {
        let input = vec![0.0f32, 1.0, 0.0];
        let output = resample_linear(&input, 1, 22050, 44100);
        assert!(
            output.len() >= 5 && output.len() <= 7,
            "expected ~6 frames, got {}",
            output.len()
        );
        // output[1] sits halfway between input[0]=0.0 and input[1]=1.0. The
        // true interpolated midpoint. output[2] lands exactly on input[1]=1.0
        // (interp factor = 0.0) so checking [2] would always equal 1.0.
        assert!(
            (output[1] - 0.5).abs() < 0.01,
            "interpolated midpoint={}",
            output[1]
        );
    }

    #[test]
    fn resample_linear_stereo_preserves_channel_interleave() {
        let input = vec![1.0f32, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
        let output = resample_linear(&input, 2, 44100, 44100);
        assert_eq!(output.len(), input.len());
        for (index, &sample) in output.iter().enumerate() {
            let expected = if index % 2 == 0 { 1.0 } else { -1.0 };
            assert!(
                (sample - expected).abs() < 1e-5,
                "idx={} got={}",
                index,
                sample
            );
        }
    }

    #[test]
    fn resample_linear_empty_input_returns_empty() {
        let output = resample_linear(&[], 1, 44100, 44100);
        assert!(output.is_empty());
    }
}
