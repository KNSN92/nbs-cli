use std::{fs::File, io::Read, path::Path};

use anyhow::Context;
use nbs_rust::io::midi::{Midi2NbsDecoder};


pub fn command_midi(midi_file: impl AsRef<Path>, nbs_file: impl AsRef<Path>) -> anyhow::Result<()> {
    let midi_path = midi_file.as_ref();
    let nbs_path = nbs_file.as_ref();

    if !midi_path.exists() {
        anyhow::bail!("MIDI file does not exist: {}", midi_path.display());
    }

    let mut buf = Vec::new();
    File::open(midi_path).context("Failed to open MIDI file")?.read_to_end(&mut buf)?;
    let nbs_data = Midi2NbsDecoder::new()
        .decode(&buf)?;
    nbs_data.save(nbs_path)?;
    println!("Successfully converted MIDI to NBS: {} -> {}", midi_path.display(), nbs_path.display());
    Ok(())

}