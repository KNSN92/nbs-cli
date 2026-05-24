use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::{info::command_info, play::command_play};

mod info;
mod play;

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
        custom_instrument_dir: Option<String>,
        // TODO: Adaptive Locatingを作成する。nbsファイルと同じディレクトリの下のディレクトリからカスタム楽器を探索し、読み込む事を試みる。また、custom_instrument_dirオプションが指定された場合はそちらを優先する。こういう時の為にInstrumentAudioProviderをトレイトにしておいたんですね〜
        #[arg(long, help = "Use adaptive custom instrument locating")]
        adaptive_locating: bool,
        #[arg(
            short,
            long,
            default_value_t = false,
            help = "Loop the song indefinitely (overrides any loop settings in the file)"
        )]
        looping: bool,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Commands::Info { file } => command_info(file),
        Commands::Play {
            file,
            custom_instrument_dir,
            adaptive_locating: _,
            looping,
        } => command_play(file, custom_instrument_dir, looping),
    }
}
