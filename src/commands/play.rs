use std::{
    borrow::Borrow,
    path::PathBuf,
    sync::{Arc, mpsc},
    thread,
};

use anyhow::{Result, anyhow, bail};
use console::style;
use cpal::{
    OutputCallbackInfo,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use crossbeam_utils::sync::Parker;
use indicatif::{ProgressBar, ProgressStyle};
use nbs_rust::{Nbs, Tick, audio::NbsAudioRenderer};
use rtrb::{Producer, RingBuffer};

use crate::io::{try_load_audio_provider, try_load_nbs_or_midi};

///! WARNING: 2の倍数である必要がある。ステレオ再生時に最後のサンプルが欠落し、ノイズが発生します。
///! WARNING: This must be a multiple of 2. When playing in stereo, the last sample will be missing and noise will occur.
const AUDIO_CHUNK_SIZE: usize = 1024;
struct AudioChunk {
    pub data: [f32; AUDIO_CHUNK_SIZE],
    pub tick: Tick,
    pub tempo: f32,
}

impl Default for AudioChunk {
    fn default() -> Self {
        Self {
            data: [0.0; AUDIO_CHUNK_SIZE],
            tick: 0,
            tempo: 0.0,
        }
    }
}

enum TickTempo {
    Tick(Tick),
    Tempo(f32),
}

pub fn command_play(
    file: String,
    custom_instrument: Option<String>,
    adaptive: bool,
    strict: bool,
    volume: u8,
    looping: bool,
) -> Result<()> {
    let mut nbs = try_load_nbs_or_midi(&file)?;
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

    let (audio_provider, failed_custom_instruments) = try_load_audio_provider(
        &mut nbs,
        custom_instrument
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(file).parent().unwrap().to_path_buf()),
        adaptive,
    )?;
    let log_level = if strict {
        style("Error").red().bold()
    } else {
        style("Warning").yellow().bold()
    };
    for ci in &failed_custom_instruments {
        eprintln!(
            "{}: missing instrument audio `{}`",
            log_level, &ci.file_name
        );
    }
    if strict && !failed_custom_instruments.is_empty() {
        bail!("Aborting due to missing instrument audio");
    }
    let nbs = Arc::new(nbs);
    let song_name = nbs.header.song_info.name.clone();
    //TODO: いずれはユーザがコンフィグでcache容量や同時prefetch可能数を指定出来るようにしたい
    let renderer = NbsAudioRenderer::builder(nbs.clone(), config.sample_rate.try_into()?)
        .audio_provider(audio_provider)
        .cache_capacity(64.try_into().unwrap())
        .prefetchable_capacity(64.try_into().unwrap())
        .build();

    let buf_len =
        (config.sample_rate as usize * config.channels as usize / AUDIO_CHUNK_SIZE * 10) as usize; // about 10 second buffer
    let (producer, mut consumer) = RingBuffer::new(buf_len);

    let (tick_send, tick_recv) = mpsc::channel();
    let mut tick_send = Some(tick_send);
    let parker = Parker::new();
    let unparker = parker.unparker().clone();
    let volume = volume as f32 / 100.0;
    if monoral {
        spawn_monoral_producer_thread(producer, renderer, parker, volume);
    } else {
        spawn_stereo_producer_thread(producer, renderer, parker, volume);
    };
    let mut chunk = AudioChunk::default();
    let mut took = AUDIO_CHUNK_SIZE;
    let mut last_tick = Tick::MAX;
    let mut last_tempo = f32::NAN;
    let data_callback = move |output: &mut [f32], _info: &OutputCallbackInfo| {
        if took >= AUDIO_CHUNK_SIZE && consumer.is_empty() && consumer.is_abandoned() {
            output.fill(0.0);
            tick_send.take();
            return;
        }
        output.fill(0.0);
        let mut filled = 0;
        while filled < output.len() {
            if took >= AUDIO_CHUNK_SIZE {
                chunk = match consumer.pop() {
                    Ok(chunk) => chunk,
                    Err(_) => break,
                };
                unparker.unpark();
                took = 0;
            }
            let to_fill = (AUDIO_CHUNK_SIZE - took).min(output.len() - filled);
            output[filled..filled + to_fill].copy_from_slice(&chunk.data[took..took + to_fill]);
            filled += to_fill;
            took += to_fill;
            if last_tick != chunk.tick {
                last_tick = chunk.tick;
                let _ = tick_send.as_ref().unwrap().send(TickTempo::Tick(chunk.tick));
            }
            if last_tempo != chunk.tempo {
                last_tempo = chunk.tempo;
                let _ = tick_send.as_ref().unwrap().send(TickTempo::Tempo(chunk.tempo));
            }
        }
    };
    let stream =
        device.build_output_stream(&config, data_callback, |e| eprintln!("Error: {}", e), None)?;
    println!("♪ Playing {} ♪", style(format!("`{}`", song_name)).yellow());
    let looping_str = if looping {
        &style(" (looping)").bold().yellow().to_string()
    } else {
        ""
    };
    let style = ProgressStyle::default_bar()
        .template(&format!(
            "[{{pos}}/{{len}} ticks] ({{msg}}tps){looping_str} [{{bar:60.green/blue}}] [{{elapsed_precise}}]"
        ))
        .unwrap()
        .progress_chars("=◎◎-");
    let progressbar = ProgressBar::new(nbs.note_blocks.ticks_len() as u64).with_style(style);
    stream.play()?;
    while let Ok(tick_tempo) = tick_recv.recv() {
        match tick_tempo {
            TickTempo::Tick(tick) => progressbar.set_position(tick as u64),
            TickTempo::Tempo(tempo) => progressbar.set_message(format!("{:.1}", tempo)),
        }
    }
    stream.pause()?;
    Ok(())
}

fn spawn_stereo_producer_thread<P: Borrow<Nbs> + Send + 'static>(
    producer: Producer<AudioChunk>,
    renderer: NbsAudioRenderer<P>,
    parker: Parker,
    volume: f32,
) {
    spawn_producer_thread(
        producer,
        renderer,
        parker,
        volume,
        |renderer, buf, volume| {
            for frame in buf.chunks_exact_mut(2) {
                if let Some([l, r]) = renderer.next() {
                    frame[0] = l * volume;
                    frame[1] = r * volume;
                } else {
                    return true;
                }
            }
            false
        },
    );
}

fn spawn_monoral_producer_thread<P: Borrow<Nbs> + Send + 'static>(
    producer: Producer<AudioChunk>,
    renderer: NbsAudioRenderer<P>,
    parker: Parker,
    volume: f32,
) {
    spawn_producer_thread(
        producer,
        renderer,
        parker,
        volume,
        |renderer, buf, volume| {
            for sample in buf {
                if let Some([l, r]) = renderer.next() {
                    *sample = (l + r) / 2.0 * volume;
                } else {
                    return true;
                }
            }
            false
        },
    );
}

fn spawn_producer_thread<P: Borrow<Nbs> + Send + 'static>(
    mut producer: Producer<AudioChunk>,
    mut renderer: NbsAudioRenderer<P>,
    parker: Parker,
    volume: f32,
    produce: impl Fn(&mut NbsAudioRenderer<P>, &mut [f32], f32) -> bool + Send + 'static,
) {
    thread::spawn(move || {
        loop {
            if producer.is_full() {
                parker.park();
                continue;
            }
            let mut chunk = AudioChunk::default();
            let ended = produce(&mut renderer, &mut chunk.data, volume);
            chunk.tick = renderer.current_tick();
            chunk.tempo = renderer.current_tempo();
            producer.push(chunk).unwrap();
            if ended {
                break;
            }
        }
    });
}
