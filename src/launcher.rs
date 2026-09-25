use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use percent_encoding::percent_decode_str;
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::config::{self, Kind};
use crate::installer;
use crate::rpc;
use crate::ui::BootstrapState;

const CLOSE_DELAY: Duration = Duration::from_millis(1200);
const CLIENT_STORAGE_DEPTH: usize = 3;
const CLIENT_STORAGE_DIRS: [&str; 4] = ["meta", "logs", "logs/archive", "LocalStorage"];
const IMAGE_PATH_CAPACITY: usize = 32768;

pub struct Launch {
    pub kind: Kind,
    pub url: Option<String>,
    args: Vec<String>,
}

impl Launch {
    pub fn parse(args: Vec<String>) -> Self {
        let from_url = args
            .iter()
            .find_map(|arg| Kind::from_url(arg).map(|kind| (kind, arg.clone())));
        let (kind, url) = match from_url {
            Some((kind, url)) => (kind, Some(url)),
            None if args.iter().any(|arg| arg == config::STUDIO_FLAG) => (Kind::Studio, None),
            None => (Kind::Player, None),
        };
        Self { kind, url, args }
    }
}

fn launch_args(kind: Kind, url: &str) -> Result<Vec<String>> {
    let params: HashMap<&str, String> = url
        .split('+')
        .filter_map(|token| token.split_once(':'))
        .map(|(key, value)| (key, percent_decode_str(value).decode_utf8_lossy().into_owned()))
        .collect();
    let param = |key: &str| {
        params
            .get(key)
            .cloned()
            .with_context(|| format!("launch link has no {key}"))
    };
    let negotiate = format!("{}/Login/Negotiate.ashx", config::BASE_URL);
    Ok(match kind {
        Kind::Player => vec![
            "-a".into(),
            negotiate,
            "-t".into(),
            param("gameinfo")?,
            "-j".into(),
            param("placelauncherurl")?,
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
    })
}

pub fn run_pipeline(launch: Launch, state: &BootstrapState) -> Result<()> {
    let kind = launch.kind;
    let label = kind.label();
    let args = launch.url.as_deref().map(|url| launch_args(kind, url)).transpose()?;
    let http = installer::http_client()?;
    let outdated = installer::launcher_outdated(&http)?;
    state.status(&format!("Upgrading {label}..."));
    if outdated {
        installer::update_launcher(&http, &launch.args, state)?;
        state.close();
        return Ok(());
    }

    let launcher = installer::install_self()?;
    installer::register_url_schemes(&launcher)?;
    installer::enforce_global_version(&http, state)?;
    let freshly_installed = installer::ensure_installed(&http, kind, state)?;
    if kind == Kind::Studio && freshly_installed {
        installer::create_studio_shortcuts(&launcher)?;
    }

    if kind == Kind::Player && args.is_none() {
        state.status("Octane is ready. Launch from the website to start.");
        state.progress(1.0);
        return Ok(());
    }
    state.status(&format!("Starting {label}..."));
    state.progress(1.0);
    let dir = kind.dir()?;
    let exe = dir.join(kind.exe());
    let args = args.unwrap_or_default();
    match kind {
        Kind::Player => {
            create_client_storage(&dir)?;
            let _ = crate::fps::spawn_window();
            rpc::spawn_watcher(&exe, &args)?;
        }
        Kind::Studio => {
            rpc::spawn_studio_watcher(&exe, &args)?;
        }
    }
    state.status("Have fun!");
    thread::sleep(CLOSE_DELAY);
    state.close();
    Ok(())
}

fn create_client_storage(client_dir: &Path) -> Result<()> {
    let storage_root = client_dir
        .ancestors()
        .nth(CLIENT_STORAGE_DEPTH)
        .context("client folder is too shallow")?;
    for dir in CLIENT_STORAGE_DIRS {
        fs::create_dir_all(storage_root.join(dir))?;
    }
    Ok(())
}

pub fn install_root_lowercase() -> Result<String> {
    Ok(config::install_root()?.to_string_lossy().to_ascii_lowercase())
}

pub fn client_already_running() -> Result<bool> {
    let root = install_root_lowercase()?;
    let client_exe = Kind::Player.exe();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error().into());
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut running = false;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more && !running {
            let name = &entry.szExeFile;
            let name_len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            running = String::from_utf16_lossy(&name[..name_len]).eq_ignore_ascii_case(client_exe)
                && process_image_under(entry.th32ProcessID, &root);
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        Ok(running)
    }
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
