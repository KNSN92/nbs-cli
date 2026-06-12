use std::{
    collections::{BTreeSet, HashMap},
    ffi::OsStr,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use console::style;
use nbs_rust::{
    Nbs,
    audio::provider::{FileAudioProvider, InstrumentAudioProvider, VanillaAudioProvider},
    instrument::InstrumentSet,
    io::midi::Midi2NbsDecoder,
};
use walkdir::WalkDir;

pub fn try_load_nbs_or_midi(path: impl AsRef<Path>) -> Result<Nbs> {
    let mut nbs = match Nbs::open(&path) {
        Ok(nbs) => nbs,
        Err(nbs_e) => {
            let mut buf = Vec::new();
            File::open(&path)?.read_to_end(&mut buf)?;
            match Midi2NbsDecoder::new().decode(&buf) {
                Ok(nbs) => nbs,
                Err(midi_e) => bail!(
                    "Failed to open given file as NBS or MIDI: \n- NBS error: {}\n- MIDI error: {}",
                    nbs_e,
                    midi_e
                ),
            }
        }
    };
    if nbs.header.song_info.name.is_empty() {
        let song_name = path
            .as_ref()
            .file_name()
            .and_then(OsStr::to_str)
            .take_if(|s| !s.is_empty())
            .unwrap_or("unnamed song");
        nbs.header.song_info.name = song_name.to_string();
    }
    Ok(nbs)
}

pub fn try_load_audio_provider(
    nbs: &mut Nbs,
    path: impl AsRef<Path>,
    adaptive: bool,
) -> Result<Box<dyn InstrumentAudioProvider + Send>> {
    if nbs.instrument_set.has_custom_instrument() {
        let dir = path.as_ref().to_path_buf();
        //TODO: Skip adaptive locating if dir is already contains all custom instruments
        let dir = if adaptive {
            let adapted_dir = adaptive_locate_custom_instrument_dir(&dir, &nbs.instrument_set)?;
            if adapted_dir.is_some() {
                nbs.instrument_set
                    .all_custom_instruments_mut()
                    .iter_mut()
                    .for_each(|ins| {
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
        Ok(Box::new(audio_provider) as Box<dyn InstrumentAudioProvider + Send>)
    } else {
        let audio_provider =
            VanillaAudioProvider::new(nbs.instrument_set.vanilla_instrument_count());
        Ok(Box::new(audio_provider) as Box<dyn InstrumentAudioProvider + Send>)
    }
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
