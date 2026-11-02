use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

use anyhow::{bail, Result};
use reqwest::blocking::Client;

use crate::site;

const EXE: &str = "OctanePlayerLauncher.exe";
const NO_UPDATE: &str = "OCTANE_NO_SELFUPDATE";


pub fn available(http: &Client) -> Result<bool> {
    if env::var_os(NO_UPDATE).is_some() {
        return Ok(false);
    }

    let latest = site::text(http, &format!("{}/setup/launcher/version.txt", crate::SITE))?;
    Ok(latest != crate::VERSION)
}


pub fn install(http: &Client, args: &[String]) -> Result<()> {
    let new = site::bytes(http, &format!("{}/setup/launcher.exe", crate::SITE))?;
    if !new.starts_with(b"MZ") {
        bail!("the downloaded launcher is not a program");
    }

    let launcher = crate::root().join(EXE);
    replace(&launcher, &new)?;
    Command::new(&launcher).args(args).env(NO_UPDATE, "1").spawn()?;
    Ok(())
}


pub fn copy_self() -> Result<PathBuf> {
    let me = env::current_exe()?;
    let launcher = crate::root().join(EXE);
    if me != launcher {
        replace(&launcher, &fs::read(&me)?)?;
    }

    remove_old_launchers()?;
    Ok(launcher)
}


fn replace(launcher: &Path, new: &[u8]) -> Result<()> {
    if launcher.exists() {
        let old = crate::root().join(format!("bootstrapper.old-{}.exe", process::id()));
        fs::rename(launcher, old)?;
    }

    fs::create_dir_all(crate::root())?;
    fs::write(launcher, new)?;
    Ok(())
}


fn remove_old_launchers() -> Result<()> {
    for entry in fs::read_dir(crate::root())? {
        let path = entry?.path();
        let name = path.file_name().unwrap().to_string_lossy();
        if name.starts_with("bootstrapper.") {
            fs::remove_file(&path).ok();
        }
    }
    Ok(())
}
