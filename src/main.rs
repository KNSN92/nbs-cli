use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::commands::{command_info, command_play};

mod commands;

#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(about = "Display information about an NBS file")]
    Info { file: String },
    #[command(about = "Play an NBS file using the default audio output device")]
    Play {
        file: String,
        #[arg(short, long, help = "Custom instrument audio directory")]
        custom_instrument: Option<String>,
        #[arg(long, help = "Use adaptive custom instrument locating")]
        adaptive: bool,
        #[arg(short, long, help = "Set the playback volume percentage (0%~200%)", default_value_t = 100, value_parser = clap::value_parser!(u8).range(0..=200))]
        volume: u8,
        #[arg(
            short,
            long,
            default_value_t = false,
            help = "Loop the song indefinitely (overrides any loop settings in the file)"
        )]
        r#loop: bool,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Commands::Info { file } => command_info(file),
        Commands::Play {
            file,
            custom_instrument,
            adaptive,
            volume,
            r#loop,
        } => command_play(
            file,
            custom_instrument,
            adaptive,
            volume,
            r#loop,
        ),
    }
}
