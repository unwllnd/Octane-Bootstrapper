use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::BaseDirs;

pub const BASE_URL: &str = "https://octane.wtf";
pub const APP_NAME: &str = "Octane";
pub const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION_MAJOR");
const LAUNCHER_EXE: &str = "OctanePlayerLauncher.exe";
pub const STUDIO_FLAG: &str = "--studio";
const CLIENT: &str = "2021";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Player,
    Studio,
}

pub const KINDS: [Kind; 2] = [Kind::Player, Kind::Studio];

impl Kind {
    pub fn from_url(url: &str) -> Option<Self> {
        let (scheme, _) = url.split_once(':')?;
        KINDS.into_iter().find(|kind| kind.scheme().eq_ignore_ascii_case(scheme))
    }

    pub fn scheme(self) -> &'static str {
        match self {
            Kind::Player => "octane-player",
            Kind::Studio => "octane-studio",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Player => "Octane",
            Kind::Studio => "Octane Studio",
        }
    }

    pub fn exe(self) -> &'static str {
        match self {
            Kind::Player => "OctanePlayer.exe",
            Kind::Studio => "RobloxStudioBeta.exe",
        }
    }

    pub fn dir(self) -> Result<PathBuf> {
        let folder = match self {
            Kind::Player => "clients",
            Kind::Studio => "Studio",
        };
        Ok(install_root()?.join(folder).join(CLIENT))
    }

    pub fn marker_file(self) -> Result<PathBuf> {
        let name = match self {
            Kind::Player => format!("INSTALLED-{CLIENT}"),
            Kind::Studio => format!("INSTALLED-studio-{CLIENT}"),
        };
        Ok(state_dir()?.join(name))
    }

    fn setup_url(self) -> String {
        match self {
            Kind::Player => format!("{BASE_URL}/setup/{CLIENT}"),
            Kind::Studio => format!("{BASE_URL}/setup/studio/{CLIENT}"),
        }
    }

    pub fn version_url(self) -> String {
        format!("{}/version.txt", self.setup_url())
    }

    pub fn bundle_url(self, version: &str) -> String {
        let suffix = match self {
            Kind::Player => "client",
            Kind::Studio => "studio",
        };
        format!("{}/{version}-{suffix}.zip", self.setup_url())
    }
}

pub fn install_root() -> Result<PathBuf> {
    let dirs = BaseDirs::new().context("no LOCALAPPDATA")?;
    Ok(dirs.data_local_dir().join(APP_NAME))
}

pub fn launcher_exe() -> Result<PathBuf> {
    Ok(install_root()?.join(LAUNCHER_EXE))
}

fn state_dir() -> Result<PathBuf> {
    Ok(install_root()?.join("state"))
}

pub fn global_version_file() -> Result<PathBuf> {
    Ok(state_dir()?.join("GLOBAL-VERSION"))
}

pub fn launcher_url() -> String {
    format!("{BASE_URL}/setup/launcher.exe")
}

pub fn launcher_version_url() -> String {
    format!("{BASE_URL}/setup/launcher/version.txt")
}

pub fn global_version_url() -> String {
    format!("{BASE_URL}/version")
}
