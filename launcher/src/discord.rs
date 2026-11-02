use std::error::Error;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use discord_rich_presence::activity::{Activity, Assets, Button, Party, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde_json::{json, Value};

use crate::site;

const APP_ID: &str = "1551561149126942810";
const IMAGE: &str = "https://octane.wtf/img/octane-rpc-v3.png";
const REFRESH: Duration = Duration::from_secs(15);

pub struct Mode {
    name: &'static str,
    details: &'static str,
    title: &'static str,
}

pub const PLAY: Mode = Mode {
    name: "play",
    details: "Playing Octane",
    title: "Octane",
};

pub const STUDIO: Mode = Mode {
    name: "studio",
    details: "Editing in Studio",
    title: "Octane Studio",
};

struct Presence {
    discord: Option<DiscordIpcClient>,
    shown: Option<Value>,
    started: i64,
}


pub fn show(mode: Mode, ticket: Option<String>, place: Option<String>) -> Result<()> {
    let http = site::client(Duration::from_secs(8))?;
    let mut url = format!("{}/rich-presence/activity?mode={}", crate::SITE, mode.name);
    if let Some(place) = place {
        url.push_str(&format!("&placeId={place}"));
    }

    thread::spawn(move || keep_showing(&mode, &http, &url, ticket.as_deref()));
    Ok(())
}


fn keep_showing(mode: &Mode, http: &Client, url: &str, ticket: Option<&str>) {
    let mut activity = json!({
        "details": mode.details,
        "largeImage": IMAGE,
        "largeText": mode.title,
    });
    let mut token = None;
    let mut presence = Presence::new();

    loop {
        if let Some(fresh) = fetch(http, url, ticket, &mut token) {
            activity = fresh;
        }
        presence.show(&activity);
        thread::sleep(REFRESH);
    }
}


fn fetch(
    http: &Client,
    url: &str,
    ticket: Option<&str>,
    token: &mut Option<String>,
) -> Option<Value> {
    let request = match (token.as_deref(), ticket) {
        (Some(token), _) => http.get(url).bearer_auth(token),
        (None, Some(ticket)) => http.get(url).header("X-Octane-Ticket", ticket),
        (None, None) => return None,
    };

    let response = request.send().ok()?;
    if response.status() == StatusCode::UNAUTHORIZED {
        *token = None;
        return None;
    }

    let body: Value = response.error_for_status().ok()?.json().ok()?;
    if let Some(fresh) = body["token"].as_str() {
        *token = Some(fresh.to_string());
    }
    body.get("activity").filter(|activity| activity.is_object()).cloned()
}


impl Presence {
    fn new() -> Presence {
        let started = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        Presence { discord: None, shown: None, started }
    }


    fn show(&mut self, activity: &Value) {
        if self.discord.is_none() {
            self.discord = connect();
            self.shown = None;
        }

        let Some(discord) = &mut self.discord else { return };
        if self.shown.as_ref() == Some(activity) {
            return;
        }

        match set_activity(discord, activity, self.started) {
            Ok(()) => self.shown = Some(activity.clone()),
            Err(_) => self.discord = None,
        }
    }
}


fn connect() -> Option<DiscordIpcClient> {
    let mut discord = DiscordIpcClient::new(APP_ID).ok()?;
    discord.connect().ok()?;
    Some(discord)
}


fn set_activity(
    discord: &mut DiscordIpcClient,
    json: &Value,
    started: i64,
) -> Result<(), Box<dyn Error>> {
    let text = |key: &str| json[key].as_str().filter(|text| !text.is_empty());

    let mut assets = Assets::new();
    if let Some(image) = text("largeImage") {
        assets = assets.large_image(image);
    }
    if let Some(caption) = text("largeText") {
        assets = assets.large_text(caption);
    }
    if let Some(image) = text("smallImage") {
        assets = assets.small_image(image);
    }
    if let Some(caption) = text("smallText") {
        assets = assets.small_text(caption);
    }

    let mut activity = Activity::new().assets(assets).timestamps(Timestamps::new().start(started));
    if let Some(details) = text("details") {
        activity = activity.details(details);
    }
    if let Some(state) = text("state") {
        activity = activity.state(state);
    }
    if let (Some(id), Some(size)) = (text("partyId"), party_size(json)) {
        activity = activity.party(Party::new().id(id).size(size));
    }

    let buttons = buttons(json);
    if !buttons.is_empty() {
        activity = activity.buttons(buttons);
    }
    discord.set_activity(activity)
}


fn party_size(json: &Value) -> Option<[i32; 2]> {
    let current = json["party"][0].as_i64()?;
    let max = json["party"][1].as_i64()?;
    Some([current as i32, max as i32])
}


fn buttons(json: &Value) -> Vec<Button<'_>> {
    let Some(listed) = json["buttons"].as_array() else { return Vec::new() };
    let mut buttons = Vec::new();
    for button in listed {
        if let (Some(label), Some(url)) = (button["label"].as_str(), button["url"].as_str()) {
            buttons.push(Button::new(label, url));
        }
    }
    buttons.truncate(2);
    buttons
}
