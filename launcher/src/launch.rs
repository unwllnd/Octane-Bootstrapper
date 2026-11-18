use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use percent_encoding::percent_decode_str;
use reqwest::blocking::Client;
use windows::core::{s, w};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::package::{self, Package, CLIENT, STUDIO, VOICE};
use crate::shell;
use crate::site;
use crate::update;
use crate::watch;
use crate::window::{self, Status};

enum Launch {
    Game(String),
    App,
    Studio(Option<String>),
}


pub fn run(args: Vec<String>) -> Result<()> {
    let launch = Launch::parse(&args);
    if matches!(launch, Launch::Game(_)) && watch::client_running() {
        return Ok(());
    }

    let title = launch.title();
    window::show(title, move |status| install_and_start(launch, &args, status))
}


fn install_and_start(launch: Launch, args: &[String], status: &Status) -> Result<()> {
    status.text(&format!("Starting {}", launch.title()));
    let http = site::client(Duration::from_secs(30))?;

    if update::available(&http)? {
        status.text(&format!("Upgrading {}", launch.title()));
        return update::install(&http, args);
    }

    let launcher = update::copy_self()?;
    shell::register_links(&launcher)?;
    package::wipe_outdated_client(&http, status)?;

    match launch {
        Launch::Game(link) => {
            let args = client_args(&link)?;
            install_client(&http, &launcher, status)?;
            install_voice(&http, status);
            start(watch::CLIENT_FLAG, &CLIENT, &args, status)
        }
        Launch::App => {
            install_client(&http, &launcher, status)?;
            install_voice(&http, status);
            start(watch::CLIENT_FLAG, &CLIENT, &["--app".to_string()], status)
        }
        Launch::Studio(link) => {
            let args = match link {
                Some(link) => studio_args(&link)?,
                None => Vec::new(),
            };
            install_studio(&http, &launcher, status)?;
            start(watch::STUDIO_FLAG, &STUDIO, &args, status)
        }
    }
}


fn install_client(http: &Client, launcher: &Path, status: &Status) -> Result<()> {
    let first_time = !CLIENT.installed();
    package::install(http, &CLIENT, status)?;
    if first_time {
        shell::save_shortcut("Octane.lnk", launcher, "", launcher)?;
    }

    if !STUDIO.installed() {
        install_studio(http, launcher, status)?;
    }

    for dir in ["meta", r"logs\archive", "LocalStorage"] {
        fs::create_dir_all(crate::local_app_data().join(dir))?;
    }
    Ok(())
}


fn install_studio(http: &Client, launcher: &Path, status: &Status) -> Result<()> {
    let first_time = !STUDIO.installed();
    package::install(http, &STUDIO, status)?;
    if first_time {
        shell::save_shortcut("Octane Studio.lnk", launcher, "--studio", &STUDIO.exe())?;
    }
    Ok(())
}


fn install_voice(http: &Client, status: &Status) {
    if voice_supported() {
        package::install(http, &VOICE, status).ok();
    }
}


fn voice_supported() -> bool {
    unsafe {
        match LoadLibraryW(w!("bcryptprimitives.dll")) {
            Ok(library) => GetProcAddress(library, s!("ProcessPrng")).is_some(),
            Err(_) => false,
        }
    }
}


fn start(flag: &str, package: &Package, args: &[String], status: &Status) -> Result<()> {
    status.text(&format!("Starting {}", package.name));
    status.progress(1.0);

    Command::new(env::current_exe()?).arg(flag).arg(package.exe()).args(args).spawn()?;

    status.text("Have fun!");
    thread::sleep(Duration::from_millis(1200));
    Ok(())
}


impl Launch {
    fn parse(args: &[String]) -> Launch {
        for arg in args {
            let lower = arg.to_ascii_lowercase();
            if lower.starts_with("octane-player:") {
                return Launch::Game(arg.clone());
            }
            if lower.starts_with("octane-studio:") {
                return Launch::Studio(Some(arg.clone()));
            }
        }

        if args.iter().any(|arg| arg == "--studio") {
            return Launch::Studio(None);
        }
        Launch::App
    }


    fn title(&self) -> &'static str {
        match self {
            Launch::Studio(_) => "Octane Studio",
            _ => "Octane",
        }
    }
}


fn client_args(link: &str) -> Result<Vec<String>> {
    let fields = link_fields(link);
    let negotiate = format!("{}/Login/Negotiate.ashx", crate::SITE);
    let args = [
        "-a", &negotiate,
        "-t", field(&fields, "gameinfo")?,
        "-j", field(&fields, "placelauncherurl")?,
    ];
    Ok(args.map(String::from).to_vec())
}


fn studio_args(link: &str) -> Result<Vec<String>> {
    let fields = link_fields(link);
    let negotiate = format!("{}/Login/Negotiate.ashx", crate::SITE);
    let mut args = vec!["-url", &negotiate, "-ticket", field(&fields, "gameinfo")?];

    match fields.get("task").map(String::as_str) {
        None => {}
        Some("TryAsset") => {
            args.extend(["-task", "TryAsset", "-assetId", field(&fields, "assetid")?]);
        }
        Some(task) => {
            args.extend(["-task", task]);
            args.extend(["-placeId", field(&fields, "placeId")?]);
            args.extend(["-universeId", field(&fields, "universeId")?]);
        }
    }
    Ok(args.into_iter().map(String::from).collect())
}


fn link_fields(link: &str) -> HashMap<&str, String> {
    let mut fields = HashMap::new();
    for part in link.split('+') {
        if let Some((key, value)) = part.split_once(':') {
            fields.insert(key, percent_decode_str(value).decode_utf8_lossy().into_owned());
        }
    }
    fields
}


fn field<'a>(fields: &'a HashMap<&str, String>, key: &str) -> Result<&'a str> {
    let value = fields.get(key).with_context(|| format!("launch link has no {key}"))?;
    Ok(value)
}
