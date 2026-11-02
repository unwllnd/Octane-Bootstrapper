use std::collections::{HashMap, VecDeque};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use anyhow::{bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use futures::StreamExt;
use livekit::webrtc::audio_stream::native::NativeAudioStream;
use tokio::task::JoinHandle;

use crate::log;
use crate::microphone;

const PREBUFFER_MS: usize = 40;
const MAX_BUFFER_MS: usize = 200;
const TRIMMED_BUFFER_MS: usize = 60;
const PLAYED_KEEP_MS: usize = 200;
const VOLUME_RAMP_SECONDS: f32 = 0.1;

pub struct Speakers {
    pub mixer: Arc<Mutex<Mixer>>,
    pub rate: u32,
    _device: mpsc::Sender<()>,
}

pub struct Mixer {
    rate: usize,
    voices: HashMap<String, Voice>,
    played: Option<VecDeque<i16>>,
}

#[derive(Default)]
struct Voice {
    buffer: VecDeque<f32>,
    playing: bool,
    volume: [f32; 2],
    current: [f32; 2],
}


impl Speakers {
    pub fn start() -> Result<Speakers> {
        let device = cpal::default_host().default_output_device().context("no output device")?;
        let supported = device.default_output_config()?;
        let format = supported.sample_format();
        let config = supported.config();
        let rate = config.sample_rate;
        let mixer = Mixer { rate: rate as usize, voices: HashMap::new(), played: None };
        let mixer = Arc::new(Mutex::new(mixer));

        let (keep, stopped) = mpsc::channel::<()>();
        let (ready, started) = mpsc::channel::<Result<()>>();
        let output = mixer.clone();
        thread::spawn(move || match open_output(&device, config, format, output) {
            Ok(stream) => {
                ready.send(Ok(())).ok();
                stopped.recv().ok();
                drop(stream);
            }
            Err(error) => {
                ready.send(Err(error)).ok();
            }
        });

        started.recv()??;
        log!("output started at {rate} Hz, {format:?}");
        Ok(Speakers { mixer, rate, _device: keep })
    }


    pub fn play(&self, identity: String, mut stream: NativeAudioStream) -> JoinHandle<()> {
        self.mixer.lock().unwrap().voices.insert(identity.clone(), Voice::default());
        let mixer = self.mixer.clone();
        tokio::spawn(async move {
            while let Some(frame) = stream.next().await {
                mixer.lock().unwrap().receive(&identity, &frame.data);
            }
        })
    }


    pub fn remove(&self, identity: &str) {
        self.mixer.lock().unwrap().voices.remove(identity);
    }


    pub fn set_volumes(&self, volumes: &HashMap<String, [f32; 2]>) {
        let mut mixer = self.mixer.lock().unwrap();
        for (identity, voice) in &mut mixer.voices {
            voice.volume = volumes.get(identity).copied().unwrap_or([0.0, 0.0]);
        }
    }
}


impl Mixer {
    pub fn record_played(&mut self, on: bool) {
        let useful = on && self.rate == microphone::RATE as usize;
        self.played = if useful { Some(VecDeque::new()) } else { None };
    }


    pub fn take_played(&mut self, count: usize) -> Option<Vec<i16>> {
        let played = self.played.as_mut()?;
        if played.len() < count {
            return None;
        }
        Some(played.drain(..count).collect())
    }


    fn receive(&mut self, identity: &str, samples: &[i16]) {
        let Some(voice) = self.voices.get_mut(identity) else { return };
        voice.buffer.extend(samples.iter().map(|&sample| sample as f32 / 32768.0));

        if voice.buffer.len() > self.rate * MAX_BUFFER_MS / 1000 {
            let excess = voice.buffer.len() - self.rate * TRIMMED_BUFFER_MS / 1000;
            voice.buffer.drain(..excess);
        }
        if !voice.playing && voice.buffer.len() >= self.rate * PREBUFFER_MS / 1000 {
            voice.playing = true;
        }
    }


    fn next_frame(&mut self) -> [f32; 2] {
        let step = 1.0 / (self.rate as f32 * VOLUME_RAMP_SECONDS);
        let mut left = 0.0;
        let mut right = 0.0;

        for voice in self.voices.values_mut() {
            for channel in 0..2 {
                let change = voice.volume[channel] - voice.current[channel];
                voice.current[channel] += change.clamp(-step, step);
            }
            if !voice.playing {
                continue;
            }
            match voice.buffer.pop_front() {
                Some(sample) => {
                    left += sample * voice.current[0];
                    right += sample * voice.current[1];
                }
                None => voice.playing = false,
            }
        }

        let frame = [left.clamp(-1.0, 1.0), right.clamp(-1.0, 1.0)];
        self.remember_played(frame);
        frame
    }


    fn remember_played(&mut self, [left, right]: [f32; 2]) {
        let Some(played) = &mut self.played else { return };
        played.push_back(((left + right) * 0.5 * 32767.0) as i16);

        let limit = self.rate * PLAYED_KEEP_MS / 1000;
        if played.len() > limit {
            let excess = played.len() - limit;
            played.drain(..excess);
        }
    }
}


fn open_output(
    device: &Device,
    config: StreamConfig,
    format: SampleFormat,
    mixer: Arc<Mutex<Mixer>>,
) -> Result<Stream> {
    let stream = match format {
        SampleFormat::F32 => output_stream::<f32>(device, config, mixer)?,
        SampleFormat::I16 => output_stream::<i16>(device, config, mixer)?,
        other => bail!("unsupported output format {other:?}"),
    };
    stream.play()?;
    Ok(stream)
}


fn output_stream<T>(
    device: &Device,
    config: StreamConfig,
    mixer: Arc<Mutex<Mixer>>,
) -> Result<Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let stream = device.build_output_stream::<T, _, _>(
        config,
        move |data, _| fill(&mixer, data, channels),
        |error| log!("output stream error: {error}"),
        None,
    )?;
    Ok(stream)
}


fn fill<T>(mixer: &Mutex<Mixer>, data: &mut [T], channels: usize)
where
    T: SizedSample + FromSample<f32>,
{
    let mut mixer = mixer.lock().unwrap();
    for frame in data.chunks_mut(channels) {
        let [left, right] = mixer.next_frame();
        if channels == 1 {
            frame[0] = T::from_sample_((left + right) * 0.5);
            continue;
        }

        frame[0] = T::from_sample_(left);
        frame[1] = T::from_sample_(right);
        for sample in &mut frame[2..] {
            *sample = T::from_sample_(0.0f32);
        }
    }
}
