use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, SetWindowTextW};

use crate::config::{self, Kind};
use crate::installer;
use crate::launcher;

pub const WATCH_FLAG: &str = "--rpc-play";
pub const STUDIO_WATCH_FLAG: &str = "--rpc-studio";

const DISCORD_APP_ID: &str = "1551561149126942810";
const DISCORD_IMAGE: &str = "https://octane.wtf/img/octane-rpc-v3.png";
const CLIENT_RENAME: (&str, &str) = ("Roblox", "Octane");
const STUDIO_RENAME: (&str, &str) = ("Roblox Studio", "Octane Studio");
const TITLE_CAPACITY: usize = 512;
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const TICKET_FLAG: &str = "-t";
const PLACE_LAUNCHER_FLAG: &str = "-j";
const STUDIO_TICKET_FLAG: &str = "-ticket";
const STUDIO_PLACE_FLAG: &str = "-placeId";
const PRESENCE_REFRESH: Duration = Duration::from_secs(15);
const PRESENCE_TIMEOUT: Duration = Duration::from_secs(8);
const PLAY_MODE: &str = "play";
const STUDIO_MODE: &str = "studio";
const MAX_BUTTONS: usize = 2;

struct Rename<'a> {
    root: &'a str,
    from: &'static str,
    to: &'static str,
}

pub fn spawn_watcher(flag: &str, exe: &Path, args: &[String]) -> Result<()> {
    Command::new(std::env::current_exe()?).arg(flag).arg(exe).args(args).spawn()?;
    Ok(())
}

pub fn watch_and_play(args: &[String]) -> Result<()> {
    let (client, client_args) = start_watched(args)?;
    spawn_voice(client_args, client.id());
    let place_id = arg_after(client_args, PLACE_LAUNCHER_FLAG).and_then(|url| query_value(&url, "placeId"));
    let presence = Presence::spawn(PLAY_MODE, arg_after(client_args, TICKET_FLAG), place_id);
    wait_renaming(client, CLIENT_RENAME)?;
    presence.stop();
    Ok(())
}

pub fn watch_studio(args: &[String]) -> Result<()> {
    let (studio, studio_args) = start_watched(args)?;
    let place_id = arg_after(studio_args, STUDIO_PLACE_FLAG).filter(|value| is_number(value));
    let presence = Presence::spawn(STUDIO_MODE, arg_after(studio_args, STUDIO_TICKET_FLAG), place_id);
    wait_renaming(studio, STUDIO_RENAME)?;
    presence.stop();
    Ok(())
}

fn start_watched(args: &[String]) -> Result<(Child, &[String])> {
    let (exe, exe_args) = args.split_first().context("no program to watch")?;
    let exe = Path::new(exe);
    let mut command = Command::new(exe);
    command.args(exe_args).current_dir(exe.parent().context("program path has no folder")?);
    if let Some(temp) = ascii_temp() {
        command.env("TEMP", &temp).env("TMP", &temp);
    }
    Ok((command.spawn()?, exe_args))
}

fn wait_renaming(mut child: Child, (from, to): (&'static str, &'static str)) -> Result<()> {
    let root = launcher::install_root_lowercase()?;
    let rename = Rename { root: &root, from, to };
    while child.try_wait()?.is_none() {
        unsafe {
            EnumWindows(Some(rename_window), &rename as *const Rename as LPARAM);
        }
        thread::sleep(POLL_INTERVAL);
    }
    Ok(())
}

unsafe extern "system" fn rename_window(hwnd: HWND, rename: LPARAM) -> BOOL {
    let rename = &*(rename as *const Rename);
    if IsWindowVisible(hwnd) == 0 {
        return TRUE;
    }
    let mut title = [0u16; TITLE_CAPACITY];
    let len = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) as usize;
    let current = String::from_utf16_lossy(&title[..len]);
    if !current.contains(rename.from) {
        return TRUE;
    }
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if launcher::process_image_under(pid, rename.root) {
        SetWindowTextW(hwnd, crate::wide(&current.replace(rename.from, rename.to)).as_ptr());
    }
    TRUE
}

fn arg_after(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|arg| arg == flag).and_then(|index| args.get(index + 1)).cloned()
}

fn is_number(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn query_value(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    let (_, value) = query.split('&').filter_map(|pair| pair.split_once('=')).find(|(name, _)| name.eq_ignore_ascii_case(key))?;
    is_number(value).then(|| value.to_string())
}

fn ascii_temp() -> Option<PathBuf> {
    let temp = std::env::temp_dir();
    let text = temp.to_string_lossy();
    if text.is_ascii() {
        return None;
    }
    let hash = text.encode_utf16().fold(0xcbf2_9ce4_8422_2325u64, |hash, unit| (hash ^ unit as u64).wrapping_mul(0x0100_0000_01b3));
    let dir = PathBuf::from(std::env::var_os("ProgramData")?).join(config::APP_NAME).join("Temp").join(format!("{hash:016x}"));
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn spawn_voice(client_args: &[String], client_pid: u32) {
    let (Some(ticket), Ok(dir), true) = (arg_after(client_args, TICKET_FLAG), Kind::Voice.dir(), launcher::voice_supported()) else {
        return;
    };
    let exe = dir.join(Kind::Voice.exe());
    if exe.is_file() {
        let _ = Command::new(&exe).arg("--ticket").arg(ticket).arg("--client-pid").arg(client_pid.to_string()).current_dir(&dir).spawn();
    }
}

struct Presence {
    running: Arc<AtomicBool>,
    worker: thread::JoinHandle<()>,
}

impl Presence {
    fn spawn(mode: &'static str, ticket: Option<String>, place_id: Option<String>) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let flag = running.clone();
        let worker = thread::spawn(move || run_presence(mode, ticket, place_id, &flag));
        Self { running, worker }
    }

    fn stop(self) {
        self.running.store(false, Ordering::Relaxed);
        let _ = self.worker.join();
    }
}

fn run_presence(mode: &str, ticket: Option<String>, place_id: Option<String>, running: &AtomicBool) {
    let started = SystemTime::now().duration_since(UNIX_EPOCH).map(|time| time.as_secs() as i64).unwrap_or(0);
    let http = installer::http_client(PRESENCE_TIMEOUT).ok();
    let mut token = None;
    let mut discord: Option<DiscordIpcClient> = None;
    let mut shown: Option<Value> = None;
    let mut next = Instant::now();
    while running.load(Ordering::Relaxed) {
        if Instant::now() >= next {
            next = Instant::now() + PRESENCE_REFRESH;
            let fetched = http.as_ref().and_then(|http| fetch_activity(http, mode, ticket.as_deref(), place_id.as_deref(), &mut token));
            let wanted = fetched.or_else(|| shown.clone()).unwrap_or_else(|| generic_activity(mode));
            if discord.is_none() {
                discord = connect_discord();
                shown = None;
            }
            if let Some(client) = discord.as_mut() {
                if shown.as_ref() != Some(&wanted) {
                    match show_activity(client, &wanted, started) {
                        Ok(()) => shown = Some(wanted),
                        Err(_) => discord = None,
                    }
                }
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
    if let Some(mut client) = discord {
        let _ = client.clear_activity();
        let _ = client.close();
    }
}

fn connect_discord() -> Option<DiscordIpcClient> {
    let mut discord = DiscordIpcClient::new(DISCORD_APP_ID).ok()?;
    discord.connect().ok()?;
    Some(discord)
}

fn fetch_activity(http: &Client, mode: &str, ticket: Option<&str>, place_id: Option<&str>, token: &mut Option<String>) -> Option<Value> {
    let mut url = format!("{}/rich-presence/activity?mode={mode}", config::BASE_URL);
    if let Some(place_id) = place_id {
        url.push_str(&format!("&placeId={place_id}"));
    }
    let request = match (token.as_deref(), ticket) {
        (Some(token), _) => http.get(&url).bearer_auth(token),
        (None, Some(ticket)) => http.get(&url).header("X-Octane-Ticket", ticket),
        (None, None) => return None,
    };
    let response = request.send().ok()?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        *token = None;
        return None;
    }
    let body: Value = serde_json::from_str(&response.error_for_status().ok()?.text().ok()?).ok()?;
    if let Some(fresh) = body.get("token").and_then(Value::as_str) {
        *token = Some(fresh.to_string());
    }
    body.get("activity").filter(|activity| activity.is_object()).cloned()
}

fn generic_activity(mode: &str) -> Value {
    if mode == STUDIO_MODE {
        json!({"details": "Editing in Studio", "largeImage": DISCORD_IMAGE, "largeText": "Octane Studio"})
    } else {
        json!({"details": "Playing Octane", "largeImage": DISCORD_IMAGE, "largeText": config::APP_NAME})
    }
}

fn button(entry: &Value) -> Option<activity::Button<'_>> {
    Some(activity::Button::new(entry.get("label")?.as_str()?, entry.get("url")?.as_str()?))
}

fn show_activity(discord: &mut DiscordIpcClient, data: &Value, started: i64) -> Result<()> {
    let text = |key: &str| data.get(key).and_then(Value::as_str).filter(|value| !value.is_empty());
    let mut assets = activity::Assets::new();
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
    let mut playing = activity::Activity::new().assets(assets).timestamps(activity::Timestamps::new().start(started));
    if let Some(details) = text("details") {
        playing = playing.details(details);
    }
    if let Some(state) = text("state") {
        playing = playing.state(state);
    }
    let size = data.get("party").and_then(Value::as_array).and_then(|size| Some([size.first()?.as_i64()? as i32, size.get(1)?.as_i64()? as i32]));
    if let (Some(id), Some(size)) = (text("partyId"), size) {
        playing = playing.party(activity::Party::new().id(id).size(size));
    }
    let listed = data.get("buttons").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let buttons: Vec<activity::Button> = listed.iter().filter_map(button).take(MAX_BUTTONS).collect();
    if !buttons.is_empty() {
        playing = playing.buttons(buttons);
    }
    discord.set_activity(playing).map_err(|error| anyhow!(error.to_string()))
}
