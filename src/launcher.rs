use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use percent_encoding::percent_decode_str;
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};

use crate::config::{self, Kind};
use crate::installer;
use crate::rpc;
use crate::ui::BootstrapState;

const CLOSE_DELAY: Duration = Duration::from_millis(1200);
const CLIENT_STORAGE_DEPTH: usize = 3;
const CLIENT_STORAGE_DIRS: [&str; 4] = ["meta", "logs", "logs/archive", "LocalStorage"];
const CLIENT_EXES: [&str; 2] = ["octaneplayer.exe", "robloxplayerbeta.exe"];
const IMAGE_PATH_CAPACITY: usize = 32768;

pub struct Launch {
    pub kind: Kind,
    pub url: Option<String>,
    args: Vec<String>,
}

impl Launch {
    pub fn parse(args: Vec<String>) -> Self {
        let from_url = args.iter().find_map(|arg| Kind::from_url(arg).map(|kind| (kind, arg.clone())));
        let (kind, url) = match from_url {
            Some((kind, url)) => (kind, Some(url)),
            None if args.iter().any(|arg| arg == config::STUDIO_FLAG) => (Kind::Studio, None),
            None => (Kind::Player, None),
        };
        Self { kind, url, args }
    }
}

fn launch_args(kind: Kind, url: &str) -> Result<Vec<String>> {
    let pairs = url.split('+').filter_map(|token| token.split_once(':'));
    let params: HashMap<&str, String> = pairs.map(|(key, value)| (key, percent_decode_str(value).decode_utf8_lossy().into_owned())).collect();
    let param = |key: &str| params.get(key).cloned().with_context(|| format!("launch link has no {key}"));
    let negotiate = format!("{}/Login/Negotiate.ashx", config::BASE_URL);
    Ok(match kind {
        Kind::Player => vec!["-a".into(), negotiate, "-t".into(), param("gameinfo")?, "-j".into(), param("placelauncherurl")?],
        Kind::Studio if !params.contains_key("task") => vec!["-url".into(), negotiate, "-ticket".into(), param("gameinfo")?],
        Kind::Studio if params.get("task").map(String::as_str) == Some("TryAsset") => vec![
            "-url".into(),
            negotiate,
            "-ticket".into(),
            param("gameinfo")?,
            "-task".into(),
            "TryAsset".into(),
            "-assetId".into(),
            param("assetid")?,
        ],
        Kind::Studio => vec![
            "-url".into(),
            negotiate,
            "-ticket".into(),
            param("gameinfo")?,
            "-task".into(),
            param("task")?,
            "-placeId".into(),
            param("placeId")?,
            "-universeId".into(),
            param("universeId")?,
        ],
        Kind::Voice => Vec::new(),
    })
}

pub fn run_pipeline(launch: Launch, state: &BootstrapState) -> Result<()> {
    let kind = launch.kind;
    let label = kind.label();
    let args = launch.url.as_deref().map(|url| launch_args(kind, url)).transpose()?;
    let starting = format!("Starting {label}");
    state.status(&starting);
    let http = installer::http_client(installer::TIMEOUT)?;
    if installer::launcher_outdated(&http)? {
        state.status(&format!("Upgrading {label}"));
        installer::update_launcher(&http, &launch.args, state)?;
        state.close();
        return Ok(());
    }

    let launcher = installer::install_self()?;
    installer::clear_leftovers(&config::install_root()?);
    installer::register_url_schemes(&launcher)?;
    installer::enforce_global_version(&http, state)?;
    let freshly_installed = installer::ensure_installed(&http, kind, state)?;
    if kind == Kind::Studio && freshly_installed {
        installer::create_studio_shortcuts(&launcher)?;
    }
    if kind == Kind::Player && !installer::is_installed(Kind::Studio)? {
        installer::ensure_installed(&http, Kind::Studio, state)?;
        installer::create_studio_shortcuts(&launcher)?;
    }
    if kind == Kind::Player && args.is_some() && voice_supported() {
        let _ = installer::ensure_installed(&http, Kind::Voice, state);
    }

    let app = kind == Kind::Player && args.is_none();
    if kind == Kind::Player {
        installer::ensure_player_shortcuts(&launcher)?;
    }
    state.status(&starting);
    state.progress(1.0);
    let dir = kind.dir()?;
    let exe = dir.join(kind.exe());
    let args = if app { vec![config::APP_FLAG.to_string()] } else { args.unwrap_or_default() };
    match kind {
        Kind::Player => {
            create_client_storage(&dir)?;
            if !app { let _ = crate::fps::spawn_window(); }
            rpc::spawn_watcher(rpc::WATCH_FLAG, &exe, &args)?;
        }
        Kind::Studio => rpc::spawn_watcher(rpc::STUDIO_WATCH_FLAG, &exe, &args)?,
        Kind::Voice => {}
    }
    state.status("Have fun!");
    thread::sleep(CLOSE_DELAY);
    state.close();
    Ok(())
}

fn create_client_storage(client_dir: &Path) -> Result<()> {
    let storage_root = client_dir.ancestors().nth(CLIENT_STORAGE_DEPTH).context("client folder is too shallow")?;
    for dir in CLIENT_STORAGE_DIRS {
        fs::create_dir_all(storage_root.join(dir))?;
    }
    Ok(())
}

pub fn voice_supported() -> bool {
    let dll = crate::wide("bcryptprimitives.dll");
    unsafe {
        let module = LoadLibraryW(dll.as_ptr());
        module != 0 && GetProcAddress(module, b"ProcessPrng\0".as_ptr()).is_some()
    }
}

pub fn install_root_lowercase() -> Result<String> {
    Ok(config::install_root()?.to_string_lossy().to_ascii_lowercase())
}

pub fn client_pids() -> Result<Vec<u32>> {
    let root = install_root_lowercase()?;
    let mut pids = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error().into());
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            let name = crate::from_wide(&entry.szExeFile).to_ascii_lowercase();
            if CLIENT_EXES.contains(&name.as_str()) && process_image_under(entry.th32ProcessID, &root) {
                pids.push(entry.th32ProcessID);
            }
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    Ok(pids)
}

pub unsafe fn process_image_under(pid: u32, root: &str) -> bool {
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if process == 0 {
        return false;
    }
    let mut path = vec![0u16; IMAGE_PATH_CAPACITY];
    let mut len = path.len() as u32;
    let found = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut len) != 0;
    CloseHandle(process);
    found && String::from_utf16_lossy(&path[..len as usize]).to_ascii_lowercase().starts_with(root)
}
