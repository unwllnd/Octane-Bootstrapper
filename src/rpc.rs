use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, SetWindowTextW,
};

use crate::config;
use crate::launcher;

pub const WATCH_FLAG: &str = "--rpc-play";
pub const STUDIO_WATCH_FLAG: &str = "--rpc-studio";

const DISCORD_APP_ID: &str = "1551561149126942810";
const DISCORD_IMAGE: &str = "octane";
const DISCORD_DETAILS: &str = "Playing Octane";
const CLIENT_TITLE: &str = "Roblox";
const STUDIO_TITLE_NEEDLE: &str = "Roblox Studio";
const STUDIO_TITLE_REPLACEMENT: &str = "Octane Studio";
const STUDIO_TITLE_CAPACITY: usize = 512;
const TITLE_CAPACITY: usize = 8;
const POLL_INTERVAL: Duration = Duration::from_secs(1);

pub fn spawn_watcher(client_exe: &Path, client_args: &[String]) -> Result<()> {
    Command::new(std::env::current_exe()?)
        .arg(WATCH_FLAG)
        .arg(client_exe)
        .args(client_args)
        .spawn()?;
    Ok(())
}

pub fn watch_and_play(args: &[String]) -> Result<()> {
    let (client_exe, client_args) = args.split_first().context("no client path to watch")?;
    let client_exe = Path::new(client_exe);
    let client_dir = client_exe.parent().context("client path has no folder")?;
    let mut client = Command::new(client_exe)
        .args(client_args)
        .current_dir(client_dir)
        .spawn()?;
    let _presence = connect_presence();
    let root = launcher::install_root_lowercase()?;
    while client.try_wait()?.is_none() {
        rename_client_windows(&root);
        thread::sleep(POLL_INTERVAL);
    }
    Ok(())
}

fn connect_presence() -> Option<DiscordIpcClient> {
    let mut discord = DiscordIpcClient::new(DISCORD_APP_ID).ok()?;
    discord.connect().ok()?;
    let started = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    let assets = activity::Assets::new()
        .large_image(DISCORD_IMAGE)
        .large_text(config::APP_NAME);
    let playing = activity::Activity::new()
        .details(DISCORD_DETAILS)
        .assets(assets)
        .timestamps(activity::Timestamps::new().start(started));
    discord.set_activity(playing).ok()?;
    Some(discord)
}

fn rename_client_windows(root: &String) {
    unsafe {
        EnumWindows(Some(rename_if_client), root as *const String as LPARAM);
    }
}

unsafe extern "system" fn rename_if_client(hwnd: HWND, root: LPARAM) -> BOOL {
    if IsWindowVisible(hwnd) == 0 {
        return TRUE;
    }
    let mut title = [0u16; TITLE_CAPACITY];
    let len = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) as usize;
    if String::from_utf16_lossy(&title[..len]) != CLIENT_TITLE {
        return TRUE;
    }
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if launcher::process_image_under(pid, &*(root as *const String)) {
        let renamed: Vec<u16> = config::APP_NAME.encode_utf16().chain([0]).collect();
        SetWindowTextW(hwnd, renamed.as_ptr());
    }
    TRUE
}


pub fn spawn_studio_watcher(studio_exe: &Path, studio_args: &[String]) -> Result<()> {
    Command::new(std::env::current_exe()?)
        .arg(STUDIO_WATCH_FLAG)
        .arg(studio_exe)
        .args(studio_args)
        .spawn()?;
    Ok(())
}

pub fn watch_studio(args: &[String]) -> Result<()> {
    let (studio_exe, studio_args) = args.split_first().context("no studio path to watch")?;
    let studio_exe = Path::new(studio_exe);
    let studio_dir = studio_exe.parent().context("studio path has no folder")?;
    let mut studio = Command::new(studio_exe)
        .args(studio_args)
        .current_dir(studio_dir)
        .spawn()?;
    let root = launcher::install_root_lowercase()?;
    while studio.try_wait()?.is_none() {
        rename_studio_windows(&root);
        thread::sleep(POLL_INTERVAL);
    }
    Ok(())
}

fn rename_studio_windows(root: &String) {
    unsafe {
        EnumWindows(Some(rename_if_studio), root as *const String as LPARAM);
    }
}

unsafe extern "system" fn rename_if_studio(hwnd: HWND, root: LPARAM) -> BOOL {
    if IsWindowVisible(hwnd) == 0 {
        return TRUE;
    }
    let mut title = [0u16; STUDIO_TITLE_CAPACITY];
    let len = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) as usize;
    let current = String::from_utf16_lossy(&title[..len]);
    if !current.contains(STUDIO_TITLE_NEEDLE) {
        return TRUE;
    }
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if launcher::process_image_under(pid, &*(root as *const String)) {
        let updated = current.replace(STUDIO_TITLE_NEEDLE, STUDIO_TITLE_REPLACEMENT);
        let renamed: Vec<u16> = updated.encode_utf16().chain([0]).collect();
        SetWindowTextW(hwnd, renamed.as_ptr());
    }
    TRUE
}
