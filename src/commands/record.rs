use std::{fs::File, path::PathBuf};

use anyhow::Result;
use hound::{SampleFormat, WavSpec, WavWriter};
use nbs_rust::audio::{NbsAudioRenderer, SampleRate};

use crate::io::{try_load_audio_provider, try_load_nbs_or_midi};


pub fn command_record(
    file: String,
    output: String,
    custom_instrument: Option<String>,
    adaptive: bool,
    volume: u8,
    sample_rate: SampleRate,
) -> Result<()> {
    let mut nbs = try_load_nbs_or_midi(&file)?;
    if nbs.header.song_meta.looping.enabled && nbs.header.song_meta.looping.count.is_none() {
        nbs.header.song_meta.looping.enabled = false;
    }
    let audio_provider = try_load_audio_provider(&mut nbs, custom_instrument.map(PathBuf::from).unwrap_or_else(|| PathBuf::from(file).parent().unwrap().to_path_buf()), adaptive)?;
    let renderer = NbsAudioRenderer::builder(nbs)
        .sample_rate(sample_rate)
        .audio_provider(audio_provider)
        .build();

    let mut wav_file = File::create(output)?;
    let mut writer = WavWriter::new(&mut wav_file, WavSpec {
        channels: 2,
        sample_rate: sample_rate.get(),
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    })?;
    let volume = volume as f32 / 100.0;
    for [mut sample_l, mut sample_r] in renderer {
        sample_l *= volume;
        sample_r *= volume;
        writer.write_sample(sample_l)?;
        writer.write_sample(sample_r)?;
    }
    Ok(())
}