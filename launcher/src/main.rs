#![windows_subsystem = "windows"]

mod discord;
mod fps;
mod launch;
mod package;
mod shell;
mod site;
mod tls;
mod update;
mod watch;
mod window;

use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::Result;

const SITE: &str = "https://octane.wtf";
const VERSION: &str = env!("CARGO_PKG_VERSION");


fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some(watch::CLIENT_FLAG) => watch::client(&args[1..]),
        Some(watch::STUDIO_FLAG) => watch::studio(&args[1..]),
        _ => launch::run(args),
    }
}


fn root() -> PathBuf {
    local_app_data().join("Octane")
}


fn local_app_data() -> PathBuf {
    let local = env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA is not set");
    PathBuf::from(local)
}


fn load_state(name: &str) -> Option<String> {
    fs::read_to_string(root().join("state").join(name)).ok()
}


fn save_state(name: &str, value: &str) -> Result<()> {
    let state = root().join("state");
    fs::create_dir_all(&state)?;
    fs::write(state.join(name), value)?;
    Ok(())
}
