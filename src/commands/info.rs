use anyhow::Result;
use console::Term;

use crate::io::try_load_nbs_or_midi;

pub fn command_info(file: String) -> Result<()> {
    let nbs = try_load_nbs_or_midi(&file)?;
    let term = Term::stdout();
    term.clear_screen()?;
    let style = term.style().green().bold();
    term.write_line(
        &style
            .apply_to(format!(
                "========== '{}' ==========\n",
                nbs.header.song_info.name
            ))
            .to_string(),
    )?;
    let style = term.style().blue();
    term.write_line(&style.apply_to(nbs.header.song_info.description).to_string())?;
    let style = term.style().yellow();
    term.write_line(
        &style
            .apply_to(format!("\nAuthor: {}", nbs.header.song_info.author))
            .to_string(),
    )?;
    term.write_line(
        &style
            .apply_to(format!(
                "Original Author: {}",
                nbs.header.song_info.original_author
            ))
            .to_string(),
    )?;
    term.write_line(
        &style
            .apply_to(format!(
                "Notes: {}",
                nbs.note_blocks
                    .inner_layer_notes()
                    .iter()
                    .map(|notes| notes.1.len())
                    .sum::<usize>()
            ))
            .to_string(),
    )?;
    term.write_line(
        &style
            .apply_to(format!(
                "Instruments: {}{}",
                nbs.instrument_set.instrument_count(),
                format!(
                    " ({})",
                    match (
                        nbs.instrument_set.vanilla_instrument_count(),
                        nbs.instrument_set.custom_instrument_count()
                    ) {
                        (0, 0) => "no instruments".to_string(),
                        (0, _) => "custom".to_string(),
                        (_, 0) => "vanilla".to_string(),
                        (v, c) => format!("{} vanilla, {} custom", v, c),
                    }
                )
            ))
            .to_string(),
    )?;
    term.write_line(
        &style
            .apply_to(format!("Layers: {}", nbs.note_blocks.layer_count()))
            .to_string(),
    )?;
    term.write_line(
        &style
            .apply_to(format!("Ticks: {} ticks", nbs.note_blocks.ticks_len()))
            .to_string(),
    )?;
    term.write_line(
        &style
            .apply_to(format!(
                "Tempo: {} t/s ({} BPM)",
                nbs.header.song_meta.tempo,
                (nbs.header.song_meta.tempo * 15.0).round()
            ))
            .to_string(),
    )?;
    Ok(())
}
