use std::{ffi::OsStr, fs::File, io::Read, path::Path};

use anyhow::{Result, bail};
use nbs_rust::{Nbs, io::midi::decoder::decode_from_midi};


pub fn try_load_nbs_or_midi(path: impl AsRef<Path>) -> Result<Nbs> {
    let mut nbs = match Nbs::open(&path) {
        Ok(nbs) => nbs
        ,
        Err(nbs_e) => {
            let mut buf = Vec::new();
            File::open(&path)?.read_to_end(&mut buf)?;
            match decode_from_midi(&buf) {
                Ok(nbs) => {
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
    if nbs.header.song_info.name.is_empty() {
        let song_name = path.as_ref()
            .file_name()
            .and_then(OsStr::to_str)
            .take_if(|s| !s.is_empty())
            .unwrap_or("unnamed song");
        nbs.header.song_info.name = song_name.to_string();
    }
    Ok(nbs)
}