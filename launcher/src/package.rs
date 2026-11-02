use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use zip::ZipArchive;

use crate::site;
use crate::window::Status;

pub struct Package {
    pub name: &'static str,
    folder: &'static str,
    exe: &'static str,
    setup: &'static str,
    zip: &'static str,
    marker: &'static str,
}

pub const CLIENT: Package = Package {
    name: "Octane",
    folder: r"clients\2021",
    exe: "OctanePlayer.exe",
    setup: "setup/2021",
    zip: "client",
    marker: "INSTALLED-2021",
};

pub const STUDIO: Package = Package {
    name: "Octane Studio",
    folder: r"Studio\2021",
    exe: "RobloxStudioBeta.exe",
    setup: "setup/studio/2021",
    zip: "studio",
    marker: "INSTALLED-studio-2021",
};

pub const VOICE: Package = Package {
    name: "Octane Voice",
    folder: "voice",
    exe: "OctaneVoice.exe",
    setup: "setup/voice",
    zip: "voice",
    marker: "INSTALLED-voice",
};


impl Package {
    pub fn dir(&self) -> PathBuf {
        crate::root().join(self.folder)
    }


    pub fn exe(&self) -> PathBuf {
        self.dir().join(self.exe)
    }


    pub fn installed(&self) -> bool {
        crate::load_state(self.marker).is_some() && self.exe().is_file()
    }


    fn download_path(&self, version: &str) -> PathBuf {
        crate::root().join(format!("{}.{version}.zip.partial", self.folder))
    }
}


pub fn install(http: &Client, package: &Package, status: &Status) -> Result<()> {
    let latest = site::text(http, &format!("{}/{}/version.txt", crate::SITE, package.setup))?;
    let installed = crate::load_state(package.marker);
    if installed.as_deref() == Some(latest.as_str()) && package.exe().is_file() {
        return Ok(());
    }

    let verb = if installed.is_some() { "Upgrading" } else { "Installing" };
    status.text(&format!("{verb} {}", package.name));

    let dir = package.dir();
    let zip = package.download_path(&latest);
    let url = format!("{}/{}/{latest}-{}.zip", crate::SITE, package.setup, package.zip);
    fs::create_dir_all(&dir)?;
    remove_old_downloads(package, &zip)?;

    site::download(http, &url, &zip, &|done| status.progress(done * 0.7))?;
    let unpacked = unpack(&zip, &dir, &|done| status.progress(0.7 + done * 0.25));
    fs::remove_file(&zip)?;
    unpacked?;

    crate::save_state(package.marker, &latest)
}


pub fn wipe_outdated_client(http: &Client, status: &Status) -> Result<()> {
    let version = site::text(http, &format!("{}/version", crate::SITE))?;
    let known = crate::load_state("GLOBAL-VERSION");
    if known.as_deref() == Some(version.as_str()) {
        return Ok(());
    }

    if known.is_some() && CLIENT.dir().exists() {
        status.text("Updating Octane to a new version");
        fs::remove_dir_all(CLIENT.dir())?;
    }
    crate::save_state("GLOBAL-VERSION", &version)
}


fn remove_old_downloads(package: &Package, keep: &Path) -> Result<()> {
    let prefix = format!("{}.", package.dir().display());
    for entry in fs::read_dir(keep.parent().unwrap())? {
        let path = entry?.path();
        let name = path.to_string_lossy();
        if name.starts_with(&prefix) && name.ends_with(".zip.partial") && path != keep {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}


fn unpack(zip: &Path, dir: &Path, progress: &dyn Fn(f32)) -> Result<()> {
    let mut archive = ZipArchive::new(File::open(zip)?)?;
    let count = archive.len();

    for i in 0..count {
        let mut entry = archive.by_index(i)?;
        let name = entry.enclosed_name().context("the zip has a file outside its folder")?;
        let path = dir.join(name);

        if entry.is_dir() {
            fs::create_dir_all(&path)?;
        } else {
            fs::create_dir_all(path.parent().unwrap())?;
            io::copy(&mut entry, &mut File::create(&path)?)?;
        }

        progress((i + 1) as f32 / count as f32);
    }
    Ok(())
}
