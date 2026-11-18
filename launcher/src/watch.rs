use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, MAX_PATH, TRUE};
use windows::Win32::System::Threading::{
    CreateMutexW, OpenMutexW, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, SYNCHRONIZATION_SYNCHRONIZE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, SetWindowTextW,
};

use crate::discord;
use crate::fps;
use crate::package::VOICE;

pub const CLIENT_FLAG: &str = "--watch-client";
pub const STUDIO_FLAG: &str = "--watch-studio";
const CLIENT_MUTEX: PCWSTR = w!("Octane.Client");


pub fn client(args: &[String]) -> Result<()> {
    let _running = unsafe { CreateMutexW(None, false, CLIENT_MUTEX)? };
    let (exe, args) = args.split_first().context("no client to watch")?;
    let client = start(exe, args)?;

    let ticket = value_after(args, "-t");
    let place = value_after(args, "-j").and_then(|url| place_in(&url));
    discord::show(discord::PLAY, ticket.clone(), place)?;
    start_voice(ticket.as_deref(), client.id());

    if ticket.is_none() {
        return watch(client, "Roblox", "Octane");
    }

    let tray = fps::show_tray(exe, client.id());
    watch(client, "Roblox", "Octane")?;
    tray.join().ok();
    Ok(())
}


pub fn studio(args: &[String]) -> Result<()> {
    let (exe, args) = args.split_first().context("no Studio to watch")?;
    let studio = start(exe, args)?;

    let ticket = value_after(args, "-ticket");
    let place = value_after(args, "-placeId").filter(|id| is_number(id));
    discord::show(discord::STUDIO, ticket, place)?;

    watch(studio, "Roblox Studio", "Octane Studio")
}


pub fn client_running() -> bool {
    unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, CLIENT_MUTEX) }.is_ok()
}


fn start(exe: &str, args: &[String]) -> Result<Child> {
    let exe = Path::new(exe);
    let temp = ascii_temp()?;

    let child = Command::new(exe)
        .args(args)
        .current_dir(exe.parent().context("the program has no folder")?)
        .env("TEMP", &temp)
        .env("TMP", &temp)
        .spawn()?;
    Ok(child)
}


fn watch(mut program: Child, from: &str, to: &str) -> Result<()> {
    while program.try_wait()?.is_none() {
        rename_windows(from, to)?;
        thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}


fn start_voice(ticket: Option<&str>, client: u32) {
    if !VOICE.exe().is_file() {
        return;
    }

    let mut voice = Command::new(VOICE.exe());
    if let Some(ticket) = ticket {
        voice.args(["--ticket", ticket]);
    }

    let client = client.to_string();
    voice.args(["--client-pid", &client]).current_dir(VOICE.dir()).spawn().ok();
}


fn ascii_temp() -> Result<PathBuf> {
    let temp = env::temp_dir();
    let path = temp.to_string_lossy();
    if path.is_ascii() {
        return Ok(temp);
    }

    let program_data = env::var_os("ProgramData").context("ProgramData is not set")?;
    let hash = format!("{:016x}", fnv_hash(&path));
    let dir = PathBuf::from(program_data).join(r"Octane\Temp").join(hash);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}


fn fnv_hash(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for unit in text.encode_utf16() {
        hash ^= unit as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}


fn rename_windows(from: &str, to: &str) -> Result<()> {
    let root = crate::root().to_string_lossy().to_lowercase();
    for window in visible_windows()? {
        let title = window_title(window);
        if title.contains(from) && is_ours(window, &root) {
            let renamed = HSTRING::from(title.replace(from, to));
            unsafe { SetWindowTextW(window, &renamed) }.ok();
        }
    }
    Ok(())
}


fn visible_windows() -> Result<Vec<HWND>> {
    let mut windows: Vec<HWND> = Vec::new();
    unsafe { EnumWindows(Some(collect_window), LPARAM(&mut windows as *mut _ as isize))? };
    windows.retain(|&window| unsafe { IsWindowVisible(window) }.as_bool());
    Ok(windows)
}


unsafe extern "system" fn collect_window(window: HWND, windows: LPARAM) -> BOOL {
    let windows = &mut *(windows.0 as *mut Vec<HWND>);
    windows.push(window);
    TRUE
}


fn window_title(window: HWND) -> String {
    let mut title = [0; 512];
    let len = unsafe { GetWindowTextW(window, &mut title) };
    String::from_utf16_lossy(&title[..len as usize])
}


fn is_ours(window: HWND, root: &str) -> bool {
    match program_path(window) {
        Some(path) => path.to_lowercase().starts_with(root),
        None => false,
    }
}


fn program_path(window: HWND) -> Option<String> {
    let mut path = [0; MAX_PATH as usize];
    let mut len = path.len() as u32;

    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let name = PWSTR(path.as_mut_ptr());
        let found = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, name, &mut len);
        CloseHandle(process).ok()?;
        found.ok()?;
    }
    Some(String::from_utf16_lossy(&path[..len as usize]))
}


fn value_after(args: &[String], flag: &str) -> Option<String> {
    let i = args.iter().position(|arg| arg == flag)?;
    args.get(i + 1).cloned()
}


fn place_in(url: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else { continue };
        if key.eq_ignore_ascii_case("placeId") && is_number(value) {
            return Some(value.to_string());
        }
    }
    None
}


fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}
