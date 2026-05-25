use std::{
    collections::{BTreeSet, HashMap},
    ffi::OsStr,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
};

use anyhow::{Context, Result, anyhow, bail};
use console::style;
use cpal::{
    OutputCallbackInfo,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use nbs_rust::{
    Nbs,
    audio::{
        NbsAudioRenderer,
        provider::{FileAudioProvider, InstrumentAudioProvider, VanillaAudioProvider},
    },
    instrument::InstrumentSet,
    io::midi::decoder::decode_from_midi,
};
use rtrb::{Producer, RingBuffer};
use walkdir::WalkDir;

pub fn command_play(
    file: String,
    custom_instrument_dir: Option<String>,
    adaptive_locating: bool,
    volume: u8,
    looping: bool,
) -> Result<()> {
    let mut nbs = match Nbs::open(&file) {
        Ok(nbs) => nbs,
        Err(nbs_e) => {
            let mut buf = Vec::new();
            File::open(&file)?.read_to_end(&mut buf)?;
            match decode_from_midi(&buf) {
                Ok(mut nbs) => {
                    let song_name = Path::new(&file)
                        .file_prefix()
                        .and_then(OsStr::to_str)
                        .unwrap_or("Unnamed Midi Song");
                    nbs.header.song_info.name = song_name.to_string();
                    nbs
                }
                Err(midi_e) => bail!(
                    "Failed to open given file as NBS or MIDI: \n- NBS error: {}\n- MIDI error: {}",
                    nbs_e,
                    midi_e
                ),
            }
        }
    };
    if looping {
        nbs.header.song_meta.looping.enabled = true;
        nbs.header.song_meta.looping.count = None;
    } else {
        nbs.header.song_meta.looping.enabled = false;
    }

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow!("No output audio device available"))?;
    let mut config = device.default_output_config()?.config();
    let monoral = config.channels == 1;
    config.channels = config.channels.min(2);
    config.sample_rate = 48000.min(config.sample_rate);

    let audio_provider = if nbs.instrument_set.has_custom_instrument() {
        let dir = custom_instrument_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(file).parent().unwrap().to_path_buf());
        //TODO: Skip adaptive locating if dir is already contains all custom instruments
        let dir = if adaptive_locating {
            let adapted_dir = adaptive_locate_custom_instrument_dir(&dir, &nbs.instrument_set)?;
            if adapted_dir.is_some() {
                nbs.instrument_set
                    .all_custom_instruments_mut()
                    .iter_mut()
                    .for_each(|ins| {
                        //TODO: Replace unwrap to proper error handling
                        let file_name = PathBuf::from(&ins.file_name)
                            .file_name()
                            .and_then(OsStr::to_str)
                            .map(str::to_string);
                        if let Some(file_name) = file_name {
                            ins.file_name = file_name;
                        }
                    });
            }
            adapted_dir.unwrap_or(dir)
        } else {
            dir
        };
        let (audio_provider, failed_custom_instruments) = FileAudioProvider::from_directory(
            &dir,
            &nbs.instrument_set,
            nbs.header.song_meta.vanilla_instruments,
        );
        for ci in failed_custom_instruments {
            eprintln!(
                "{}: failed to load custom instrument from `{}`",
                style("warning").yellow().bold(),
                dir.join(&ci.file_name).display()
            );
        }
        Box::new(audio_provider) as Box<dyn InstrumentAudioProvider + Send>
    } else {
        let audio_provider =
            VanillaAudioProvider::new(nbs.instrument_set.vanilla_instrument_count());
        Box::new(audio_provider) as Box<dyn InstrumentAudioProvider + Send>
    };
    let song_name = nbs.header.song_info.name.clone();
    let renderer = NbsAudioRenderer::builder(nbs)
        .sample_rate(config.sample_rate.try_into()?)
        .audio_provider(audio_provider)
        .build();

    let buf_len = (config.sample_rate as f32 * config.channels as f32 * 60.0).floor() as usize; // 1 minute buffer 60.0 is seconds
    let (producer, mut consumer) = RingBuffer::<f32>::new(buf_len);

    let (end_send, end_recv) = mpsc::channel();
    let mut end_send = Some(end_send);
    let volume = volume as f32 / 100.0;
    if monoral {
        spawn_monoral_producer_thread(producer, renderer, volume);
    } else {
        spawn_stereo_producer_thread(producer, renderer, volume);
    };
    let data_callback = move |output: &mut [f32], _info: &OutputCallbackInfo| {
        if consumer.is_empty() && consumer.is_abandoned() {
            if let Some(end_send) = end_send.take() {
                let _ = end_send.send(());
            }
            output.fill(0.0);
            return;
        }

        let filled_len = consumer.pop_partial_slice(output).0.len();
        output[filled_len..].fill(0.0);

        if filled_len == 0 {
            thread::yield_now();
        }
    };
    let stream =
        device.build_output_stream(&config, data_callback, |e| eprintln!("Error: {}", e), None)?;
    println!("♪ Playing {} ♪", style(format!("`{}`", song_name)).yellow());
    stream.play()?;
    end_recv.recv()?;
    stream.pause()?;
    Ok(())
}

// TODO: marge this function and `spawn_monoral_producer_thread`
fn spawn_stereo_producer_thread(
    mut producer: Producer<f32>,
    mut renderer: NbsAudioRenderer,
    volume: f32,
) {
    thread::spawn(move || {
        let mut buf = vec![0.0; 1024].into_boxed_slice();
        let mut remaining_len = 0;
        let mut ended = false;
        loop {
            if ended && remaining_len == 0 {
                break;
            }
            if producer.is_full() {
                thread::yield_now();
                continue;
            }
            let mut produced = remaining_len;
            for frame in buf[remaining_len..].chunks_exact_mut(2) {
                if let Some([l, r]) = renderer.next() {
                    frame[0] = l * volume;
                    frame[1] = r * volume;
                    produced += 2;
                } else {
                    ended = true;
                    break;
                }
            }
            let (_, remaining) = producer.push_partial_slice(&buf[..produced]);
            remaining_len = remaining.len();
            let buf_len = buf.len();
            buf.copy_within((buf_len - remaining_len).., 0);
        }
    });
}

fn spawn_monoral_producer_thread(
    mut producer: Producer<f32>,
    mut renderer: NbsAudioRenderer,
    volume: f32,
) {
    thread::spawn(move || {
        let mut buf = vec![0.0; 1024].into_boxed_slice();
        let mut remaining_len = 0;
        let mut ended = false;
        loop {
            if ended && remaining_len == 0 {
                break;
            }
            if producer.is_full() {
                thread::yield_now();
                continue;
            }
            let mut produced = remaining_len;
            for sample in &mut buf[remaining_len..] {
                if let Some([l, r]) = renderer.next() {
                    *sample = (l + r) / 2.0 * volume;
                    produced += 1;
                } else {
                    ended = true;
                    break;
                }
            }
            let (_, remaining) = producer.push_partial_slice(&buf[..produced]);
            remaining_len = remaining.len();
            let buf_len = buf.len();
            buf.copy_within((buf_len - remaining_len).., 0);
        }
    });
}

const MAX_ADAPTIVE_LOCATING_DEPTH: usize = 5;

fn adaptive_locate_custom_instrument_dir(
    file: impl AsRef<Path>,
    instrument_set: &InstrumentSet,
) -> Result<Option<PathBuf>> {
    let custom_instrument_file_names = instrument_set
        .all_custom_instruments()
        .iter()
        .filter_map(|ci| {
            PathBuf::from(&ci.file_name)
                .file_name()
                .and_then(|s| s.to_str().map(str::to_string))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut custom_instrument_dir_candidates = HashMap::new();
    let walkdir = WalkDir::new(file)
        .max_depth(MAX_ADAPTIVE_LOCATING_DEPTH)
        .follow_links(false)
        .follow_root_links(false);
    for file in walkdir.into_iter() {
        let file = file?;
        if !file.metadata()?.is_file() {
            continue;
        }
        let file_name = file
            .file_name()
            .to_str()
            .map(str::to_string)
            .context("An invalid character contains in the custom instrument file name")?;
        if let Ok(index) = custom_instrument_file_names.binary_search(&file_name) {
            let parent_dir = file.path().parent().unwrap().to_path_buf();
            let (found, found_instruments) = custom_instrument_dir_candidates
                .entry(parent_dir)
                .or_insert((0, vec![false; custom_instrument_file_names.len()]));
            if !found_instruments[index] {
                *found += 1;
                found_instruments[index] = true;
                if *found == custom_instrument_file_names.len() {
                    return Ok(Some(file.path().parent().unwrap().to_path_buf()));
                }
            }
        }
    }
    if custom_instrument_dir_candidates.is_empty() {
        Ok(None)
    } else {
        let (best_dir, _) = custom_instrument_dir_candidates
            .into_iter()
            .max_by_key(|(_, (found_count, _))| *found_count)
            .unwrap();
        Ok(Some(best_dir))
    }
}
