use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use livekit::prelude::*;
use livekit::webrtc::audio_stream::native::NativeAudioStream;
use serde::Deserialize;
use tokio::task::JoinHandle;
use tokio::time;

use crate::log;
use crate::microphone::Microphone;
use crate::site::{Grant, Reply, Site};
use crate::speakers::Speakers;

struct Call {
    site: Site,
    room: String,
    me: String,
    speakers: Speakers,
    microphone: Option<Microphone>,
    voices: HashMap<String, JoinHandle<()>>,
    speaking: bool,
}

#[derive(Deserialize)]
struct Tick {
    #[serde(default, rename = "g")]
    volumes: HashMap<String, HashMap<String, [f32; 2]>>,
    #[serde(default, rename = "m")]
    muted: HashMap<String, bool>,
}


pub async fn join(site: &Site, grant: Grant) -> Result<()> {
    log!("joining {} as {} (speaker {})", grant.room, grant.identity, grant.speaker);
    let (room, mut events) = Room::connect(&grant.url, &grant.token, RoomOptions::default()).await?;
    let mut call = Call::start(site, &room, grant).await?;
    let mut recheck = time::interval(Duration::from_secs(5));
    recheck.tick().await;

    loop {
        tokio::select! {
            event = events.recv() => match event {
                Some(RoomEvent::Disconnected { reason }) => {
                    log!("disconnected: {reason:?}");
                    break;
                }
                Some(event) => call.handle(event),
                None => break,
            },
            _ = recheck.tick() => {
                if !call.still_granted().await {
                    break;
                }
            }
        }
    }

    call.end();
    room.close().await?;
    Ok(())
}


impl Call {
    async fn start(site: &Site, room: &Room, grant: Grant) -> Result<Call> {
        let speakers = Speakers::start()?;
        let mut microphone = None;
        if grant.speaker {
            match Microphone::start(room, speakers.mixer.clone()).await {
                Ok(started) => microphone = Some(started),
                Err(error) => log!("microphone: {error:#}"),
            }
        }

        Ok(Call {
            site: site.clone(),
            room: grant.room,
            me: grant.identity,
            speakers,
            microphone,
            voices: HashMap::new(),
            speaking: false,
        })
    }


    fn handle(&mut self, event: RoomEvent) {
        match event {
            RoomEvent::TrackSubscribed { track: RemoteTrack::Audio(track), participant, .. } => {
                self.listen(participant.identity().to_string(), track);
            }
            RoomEvent::TrackUnsubscribed { participant, .. } => {
                self.stop_listening(&participant.identity().to_string());
            }
            RoomEvent::DataReceived { payload, topic: Some(topic), .. } if topic == "octane" => {
                self.apply(&payload);
            }
            RoomEvent::ActiveSpeakersChanged { speakers } => {
                let speaking = speakers.iter().any(|who| who.identity().to_string() == self.me);
                self.set_speaking(speaking);
            }
            _ => {}
        }
    }


    fn listen(&mut self, identity: String, track: RemoteAudioTrack) {
        let stream = NativeAudioStream::new(track.rtc_track(), self.speakers.rate as i32, 1);
        let voice = self.speakers.play(identity.clone(), stream);
        if let Some(previous) = self.voices.insert(identity.clone(), voice) {
            previous.abort();
        }
        log!("listening to {identity}");
    }


    fn stop_listening(&mut self, identity: &str) {
        if let Some(voice) = self.voices.remove(identity) {
            voice.abort();
        }
        self.speakers.remove(identity);
        log!("stopped listening to {identity}");
    }


    fn apply(&mut self, payload: &[u8]) {
        let Ok(mut tick) = serde_json::from_slice::<Tick>(payload) else { return };
        self.speakers.set_volumes(&tick.volumes.remove(&self.me).unwrap_or_default());

        let muted = tick.muted.get(&self.me);
        if let (Some(microphone), Some(&muted)) = (&mut self.microphone, muted) {
            microphone.set_muted(muted);
        }
    }


    fn set_speaking(&mut self, speaking: bool) {
        if self.microphone.is_none() || speaking == self.speaking {
            return;
        }
        self.speaking = speaking;
        self.site.report_speaking(speaking);
    }


    async fn still_granted(&self) -> bool {
        match self.site.check_grant().await {
            Ok(Reply::Ok(grant)) if grant.room == self.room => true,
            Ok(_) => {
                log!("left room {}", self.room);
                false
            }
            Err(error) => {
                log!("recheck: {error:#}");
                true
            }
        }
    }


    fn end(self) {
        for voice in self.voices.values() {
            voice.abort();
        }
    }
}
