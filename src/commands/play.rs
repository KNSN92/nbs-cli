use std::{borrow::Borrow, path::PathBuf, sync::mpsc, thread};

use anyhow::{Result, anyhow};
use console::style;
use cpal::{
    OutputCallbackInfo,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use crossbeam_utils::sync::Parker;
use nbs_rust::{Nbs, audio::NbsAudioRenderer};
use rtrb::{Producer, RingBuffer};

use crate::io::{try_load_audio_provider, try_load_nbs_or_midi};

pub fn command_play(
    file: String,
    custom_instrument: Option<String>,
    adaptive: bool,
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

    let audio_provider = try_load_audio_provider(
        &mut nbs,
        custom_instrument
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(file).parent().unwrap().to_path_buf()),
        adaptive,
    )?;
    let song_name = nbs.header.song_info.name.clone();
    let renderer =
        NbsAudioRenderer::with_audio_provider(nbs, config.sample_rate.try_into()?, audio_provider);

    let buf_len = (config.sample_rate as f32 * config.channels as f32 * 60.0).floor() as usize; // 1 minute buffer 60.0 is seconds
    let (producer, mut consumer) = RingBuffer::<f32>::new(buf_len);

    let (end_send, end_recv) = mpsc::channel();
    let parker = Parker::new();
    let unparker = parker.unparker().clone();
    let mut end_send = Some(end_send);
    let volume = volume as f32 / 100.0;
    if monoral {
        spawn_monoral_producer_thread(producer, renderer, parker, volume);
    } else {
        spawn_stereo_producer_thread(producer, renderer, parker, volume);
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
        unparker.unpark();
    };
    let stream =
        device.build_output_stream(&config, data_callback, |e| eprintln!("Error: {}", e), None)?;
    println!("♪ Playing {} ♪", style(format!("`{}`", song_name)).yellow());
    stream.play()?;
    end_recv.recv()?;
    stream.pause()?;
    Ok(())
}

fn spawn_stereo_producer_thread<P: Borrow<Nbs> + Send + 'static>(
    producer: Producer<f32>,
    renderer: NbsAudioRenderer<P>,
    parker: Parker,
    volume: f32,
) {
    spawn_producer_thread(
        producer,
        renderer,
        parker,
        volume,
        |renderer, buf, remaining_len, volume, ended| {
            let mut produced = 0;
            for frame in buf[remaining_len..].chunks_exact_mut(2) {
                if let Some([l, r]) = renderer.next() {
                    frame[0] = l * volume;
                    frame[1] = r * volume;
                    produced += 2;
                } else {
                    *ended = true;
                    break;
                }
            }
            produced
        },
    );
}

fn spawn_monoral_producer_thread<P: Borrow<Nbs> + Send + 'static>(
    producer: Producer<f32>,
    renderer: NbsAudioRenderer<P>,
    parker: Parker,
    volume: f32,
) {
    spawn_producer_thread(
        producer,
        renderer,
        parker,
        volume,
        |renderer, buf, remaining_len, volume, ended| {
            let mut produced = 0;
            for sample in &mut buf[remaining_len..] {
                if let Some([l, r]) = renderer.next() {
                    *sample = (l + r) / 2.0 * volume;
                    produced += 1;
                } else {
                    *ended = true;
                    break;
                }
            }
            produced
        },
    );
}

fn spawn_producer_thread<P: Borrow<Nbs> + Send + 'static>(
    mut producer: Producer<f32>,
    mut renderer: NbsAudioRenderer<P>,
    parker: Parker,
    volume: f32,
    produce: impl Fn(&mut NbsAudioRenderer<P>, &mut [f32], usize, f32, &mut bool) -> usize + Send + 'static,
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
                parker.park();
                continue;
            }
            let produced = produce(
                &mut renderer,
                buf.as_mut(),
                remaining_len,
                volume,
                &mut ended,
            ) + remaining_len;
            let (_, remaining) = producer.push_partial_slice(&buf[..produced]);
            remaining_len = remaining.len();
            let buf_len = buf.len();
            buf.copy_within((buf_len - remaining_len).., 0);
        }
    });
}
