use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use directories::{BaseDirs, UserDirs};
use reqwest::blocking::Client;
use windows::core::{ComInterface, HSTRING};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use crate::config::{self, Kind, KINDS};

const LEGACY_PLAYER_EXE: &str = "RobloxPlayerBeta.exe";
use crate::ui::BootstrapState;

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) OctaneBootstrapper/0.1 (+https://www.octane.wtf)";
const TIMEOUT: Duration = Duration::from_secs(30);
const CHUNK_SIZE: usize = 64 * 1024;
const NO_SELF_UPDATE_ENV: &str = "OCTANE_NO_SELFUPDATE";
const EXE_MAGIC: [u8; 2] = *b"MZ";
const DOWNLOAD_END: f32 = 0.7;
const EXTRACT_END: f32 = 0.95;
const START_MENU_PROGRAMS: &str = "Microsoft\\Windows\\Start Menu\\Programs";
const STUDIO_SHORTCUT: &str = "Octane Studio.lnk";

pub fn http_client() -> Result<Client> {
    Ok(Client::builder().user_agent(USER_AGENT).timeout(TIMEOUT).build()?)
}

fn fetch_text(http: &Client, url: &str) -> Result<String> {
    let text = http.get(url).send()?.error_for_status()?.text()?;
    Ok(text.trim().to_string())
}

fn download(http: &Client, url: &str, dest: &Path, on_progress: impl Fn(f32)) -> Result<()> {
    let mut response = http.get(url).send()?.error_for_status()?;
    let total = response.content_length();
    let mut file = fs::File::create(dest).with_context(|| format!("creating {}", dest.display()))?;
    let mut chunk = vec![0; CHUNK_SIZE];
    let mut received = 0u64;
    loop {
        let read = response.read(&mut chunk)?;
        if read == 0 {
            return Ok(());
        }
        file.write_all(&chunk[..read])?;
        received += read as u64;
        if let Some(total) = total {
            on_progress(received as f32 / total as f32);
        }
    }
}

pub fn launcher_outdated(http: &Client) -> Result<bool> {
    if std::env::var_os(NO_SELF_UPDATE_ENV).is_some() {
        return Ok(false);
    }
    Ok(fetch_text(http, &config::launcher_version_url())? != config::LAUNCHER_VERSION)
}

pub fn update_launcher(http: &Client, args: &[String], state: &BootstrapState) -> Result<()> {
    let root = config::install_root()?;
    fs::create_dir_all(&root)?;
    let installed = config::launcher_exe()?;
    let staged = root.join(format!("bootstrapper.update-{}.exe", std::process::id()));
    let retired = root.join("bootstrapper.old.exe");

    if let Err(err) = download_launcher(http, &staged, state) {
        let _ = fs::remove_file(&staged);
        return Err(err);
    }
    if installed.exists() {
        clear_retired(&root);
        let aside = if retired.exists() {
            root.join(format!("bootstrapper.old-{}.exe", std::process::id()))
        } else {
            retired
        };
        fs::rename(&installed, &aside).context("moving the old launcher aside")?;
    }
    fs::rename(&staged, &installed).context("putting the new launcher in place")?;

    Command::new(&installed)
        .args(args)
        .env(NO_SELF_UPDATE_ENV, "1")
        .current_dir(&root)
        .spawn()?;
    Ok(())
}

fn clear_retired(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with("bootstrapper.old") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn download_launcher(http: &Client, dest: &Path, state: &BootstrapState) -> Result<()> {
    download(http, &config::launcher_url(), dest, |fraction| state.progress(fraction))?;
    let mut magic = [0; EXE_MAGIC.len()];
    fs::File::open(dest)?.read_exact(&mut magic)?;
    if magic != EXE_MAGIC {
        bail!("downloaded launcher is not an executable");
    }
    Ok(())
}

pub fn install_self() -> Result<PathBuf> {
    let installed = config::launcher_exe()?;
    let current = std::env::current_exe()?;
    if same_file(&current, &installed) {
        return Ok(installed);
    }
    fs::create_dir_all(config::install_root()?)?;
    let copied = fs::copy(&current, &installed);
    if copied.is_err() && !installed.is_file() {
        copied.with_context(|| format!("copying launcher to {}", installed.display()))?;
    }
    Ok(installed)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

pub fn register_url_schemes(launcher: &Path) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let launcher = launcher.display();
    for kind in KINDS {
        let base = format!("Software\\Classes\\{}", kind.scheme());

        let (scheme, _) = hkcu.create_subkey(&base)?;
        scheme.set_value("", &format!("URL:{} Protocol", kind.label()))?;
        scheme.set_value("URL Protocol", &"")?;

        let (icon, _) = hkcu.create_subkey(format!("{base}\\DefaultIcon"))?;
        icon.set_value("", &format!("\"{launcher}\",0"))?;

        let (command, _) = hkcu.create_subkey(format!("{base}\\shell\\open\\command"))?;
        command.set_value("", &format!("\"{launcher}\" \"%1\""))?;
    }
    Ok(())
}

pub fn enforce_global_version(http: &Client, state: &BootstrapState) -> Result<()> {
    let live = fetch_text(http, &config::global_version_url())?;
    let file = config::global_version_file()?;
    let known = fs::read_to_string(&file).unwrap_or_default();
    if known.trim() == live {
        return Ok(());
    }
    if !known.is_empty() {
        state.status("Updating Octane to a new version...");
        let player = Kind::Player.dir()?;
        if player.exists() {
            fs::remove_dir_all(&player).with_context(|| format!("removing {}", player.display()))?;
        }
        let marker = Kind::Player.marker_file()?;
        if marker.exists() {
            fs::remove_file(&marker)?;
        }
    }
    write_state(&file, &live)
}

fn write_state(file: &Path, value: &str) -> Result<()> {
    fs::create_dir_all(file.parent().unwrap())
        .and_then(|_| fs::write(file, value))
        .with_context(|| format!("writing {}", file.display()))
}

pub fn ensure_installed(http: &Client, kind: Kind, state: &BootstrapState) -> Result<bool> {
    let version = fetch_text(http, &kind.version_url())?;
    let marker = kind.marker_file()?;
    let installed = fs::read_to_string(&marker).unwrap_or_default();
    let dir = kind.dir()?;
    if installed.trim() == version && dir.join(kind.exe()).is_file() {
        return Ok(false);
    }

    fs::create_dir_all(&dir)?;
    let bundle = dir.with_extension("zip.partial");
    download(http, &kind.bundle_url(&version), &bundle, |fraction| {
        state.progress(fraction * DOWNLOAD_END)
    })
    .with_context(|| format!("downloading {}", kind.label()))?;
    extract(&bundle, &dir, |fraction| {
        state.progress(DOWNLOAD_END + fraction * (EXTRACT_END - DOWNLOAD_END))
    })
    .with_context(|| format!("extracting {}", kind.label()))?;
    fs::remove_file(&bundle)?;
    write_state(&marker, &version)?;
    if kind == Kind::Player {
        let _ = fs::remove_file(dir.join(LEGACY_PLAYER_EXE));
    }
    Ok(true)
}

fn extract(bundle: &Path, dir: &Path, on_progress: impl Fn(f32)) -> Result<()> {
    let mut archive = zip::ZipArchive::new(fs::File::open(bundle)?)?;
    let count = archive.len();
    for index in 0..count {
        let mut entry = archive.by_index(index)?;
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        let out = dir.join(relative);
        let is_dir = entry.is_dir();
        write_entry(&mut entry, is_dir, &out).with_context(|| format!("writing {}", out.display()))?;
        on_progress((index + 1) as f32 / count as f32);
    }
    Ok(())
}

fn write_entry(entry: &mut impl Read, is_dir: bool, out: &Path) -> io::Result<()> {
    if is_dir {
        return fs::create_dir_all(out);
    }
    fs::create_dir_all(out.parent().unwrap())?;
    io::copy(entry, &mut fs::File::create(out)?)?;
    Ok(())
}

pub fn create_studio_shortcuts(launcher: &Path) -> Result<()> {
    let studio_exe = Kind::Studio.dir()?.join(Kind::Studio.exe());
    let start_menu = BaseDirs::new().context("no APPDATA")?.data_dir().join(START_MENU_PROGRAMS);
    let user_dirs = UserDirs::new().context("no user folders")?;
    let desktop = user_dirs.desktop_dir().context("no Desktop folder")?;
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)?;
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(launcher))?;
        link.SetArguments(&HSTRING::from(config::STUDIO_FLAG))?;
        link.SetWorkingDirectory(&HSTRING::from(config::install_root()?.as_path()))?;
        link.SetIconLocation(&HSTRING::from(studio_exe.as_path()), 0)?;
        let file: IPersistFile = link.cast()?;
        for folder in [start_menu.as_path(), desktop] {
            fs::create_dir_all(folder)?;
            file.Save(&HSTRING::from(folder.join(STUDIO_SHORTCUT).as_path()), true)?;
        }
    }
    Ok(())
}
