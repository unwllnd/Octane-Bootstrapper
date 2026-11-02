use std::borrow::Cow;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use anyhow::{bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    Device, FromSample, SampleFormat, SizedSample, Stream, StreamConfig, SupportedStreamConfig,
};
use futures::executor::block_on;
use livekit::options::TrackPublishOptions;
use livekit::prelude::*;
use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::audio_source::{AudioSourceOptions, RtcAudioSource};
use livekit::webrtc::native::apm::AudioProcessingModule;
use livekit::webrtc::native::audio_resampler::AudioResampler;

use crate::log;
use crate::speakers::Mixer;

pub const RATE: u32 = 48000;
const FRAME: usize = (RATE / 100) as usize;

pub struct Microphone {
    track: LocalAudioTrack,
    control: mpsc::Sender<Message>,
    muted: bool,
}

enum Message {
    Samples(Vec<f32>),
    Muted(bool),
    Stop,
}

struct Capture {
    device: Device,
    config: SupportedStreamConfig,
    rate: u32,
    stream: Option<Stream>,
    sender: mpsc::Sender<Message>,
    mixer: Arc<Mutex<Mixer>>,
    source: NativeAudioSource,
    cleaner: AudioProcessingModule,
    resampler: AudioResampler,
    raw: Vec<f32>,
    ready: Vec<i16>,
}


impl Microphone {
    pub async fn start(room: &Room, mixer: Arc<Mutex<Mixer>>) -> Result<Microphone> {
        let source = NativeAudioSource::new(AudioSourceOptions::default(), RATE, 1, 0);
        let audio = RtcAudioSource::Native(source.clone());
        let track = LocalAudioTrack::create_audio_track("microphone", audio);
        let options = TrackPublishOptions {
            source: TrackSource::Microphone,
            dtx: true,
            red: true,
            ..Default::default()
        };
        room.local_participant().publish_track(LocalTrack::Audio(track.clone()), options).await?;
        track.mute();

        let (control, messages) = mpsc::channel();
        let (ready, started) = mpsc::channel();
        let sender = control.clone();
        thread::spawn(move || match Capture::open(sender, mixer, source) {
            Ok(capture) => {
                ready.send(Ok(capture.rate)).ok();
                capture.run(messages);
            }
            Err(error) => {
                ready.send(Err(error)).ok();
            }
        });

        let rate = started.recv()??;
        log!("microphone ready at {rate} Hz");
        Ok(Microphone { track, control, muted: true })
    }


    pub fn set_muted(&mut self, muted: bool) {
        if muted == self.muted {
            return;
        }

        self.muted = muted;
        if muted {
            self.track.mute();
        } else {
            self.track.unmute();
        }
        self.control.send(Message::Muted(muted)).ok();
        log!("microphone {}", if muted { "muted" } else { "live" });
    }
}


impl Drop for Microphone {
    fn drop(&mut self) {
        self.control.send(Message::Stop).ok();
    }
}


impl Capture {
    fn open(
        sender: mpsc::Sender<Message>,
        mixer: Arc<Mutex<Mixer>>,
        source: NativeAudioSource,
    ) -> Result<Capture> {
        let device = cpal::default_host().default_input_device().context("no input device")?;
        let config = device.default_input_config()?;
        let format = config.sample_format();
        if !matches!(format, SampleFormat::F32 | SampleFormat::I16) {
            bail!("unsupported input format {format:?}");
        }

        mixer.lock().unwrap().record_played(true);
        Ok(Capture {
            device,
            rate: config.sample_rate(),
            config,
            stream: None,
            sender,
            mixer,
            source,
            cleaner: AudioProcessingModule::new(true, true, true, true),
            resampler: AudioResampler::default(),
            raw: Vec::new(),
            ready: Vec::new(),
        })
    }


    fn run(mut self, messages: mpsc::Receiver<Message>) {
        while let Ok(message) = messages.recv() {
            match message {
                Message::Muted(true) => self.mute(),
                Message::Muted(false) => self.unmute(),
                Message::Samples(samples) => self.feed(samples),
                Message::Stop => break,
            }
        }
        self.mixer.lock().unwrap().record_played(false);
    }


    fn mute(&mut self) {
        self.stream = None;
        self.raw.clear();
        self.ready.clear();
    }


    fn unmute(&mut self) {
        if self.stream.is_some() {
            return;
        }
        match open_input(&self.device, &self.config, self.sender.clone()) {
            Ok(stream) => self.stream = Some(stream),
            Err(error) => log!("microphone: {error:#}"),
        }
    }


    fn feed(&mut self, samples: Vec<f32>) {
        if self.stream.is_none() {
            return;
        }

        let chunk = (self.rate / 100) as usize;
        self.raw.extend(samples);
        while self.raw.len() >= chunk {
            let samples: Vec<i16> = self.raw.drain(..chunk).map(to_i16).collect();
            if self.rate == RATE {
                self.ready.extend(samples);
                continue;
            }
            let resampled =
                self.resampler.remix_and_resample(&samples, chunk as u32, 1, self.rate, 1, RATE);
            self.ready.extend_from_slice(resampled);
        }

        while self.ready.len() >= FRAME {
            let frame: Vec<i16> = self.ready.drain(..FRAME).collect();
            self.send(frame);
        }
    }


    fn send(&mut self, mut frame: Vec<i16>) {
        let played = self.mixer.lock().unwrap().take_played(FRAME);
        if let Some(mut played) = played {
            self.cleaner.process_reverse_stream(&mut played, RATE as i32, 1).ok();
        }
        self.cleaner.process_stream(&mut frame, RATE as i32, 1).ok();

        let audio = AudioFrame {
            data: Cow::Owned(frame),
            sample_rate: RATE,
            num_channels: 1,
            samples_per_channel: FRAME as u32,
        };
        block_on(self.source.capture_frame(&audio)).ok();
    }
}


fn open_input(
    device: &Device,
    config: &SupportedStreamConfig,
    sender: mpsc::Sender<Message>,
) -> Result<Stream> {
    let stream = match config.sample_format() {
        SampleFormat::F32 => input_stream::<f32>(device, config.config(), sender)?,
        _ => input_stream::<i16>(device, config.config(), sender)?,
    };
    stream.play()?;
    Ok(stream)
}


fn input_stream<T>(
    device: &Device,
    config: StreamConfig,
    sender: mpsc::Sender<Message>,
) -> Result<Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels as usize;
    let stream = device.build_input_stream::<T, _, _>(
        config,
        move |data, _| {
            sender.send(Message::Samples(to_mono(data, channels))).ok();
        },
        |error| log!("input stream error: {error}"),
        None,
    )?;
    Ok(stream)
}


fn to_mono<T>(data: &[T], channels: usize) -> Vec<f32>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let mut mono = Vec::with_capacity(data.len() / channels);
    for frame in data.chunks(channels) {
        let sum: f32 = frame.iter().map(|&sample| f32::from_sample_(sample)).sum();
        mono.push(sum / channels as f32);
    }
    mono
}


fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * 32767.0) as i16
}
