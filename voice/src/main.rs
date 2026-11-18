#![windows_subsystem = "windows"]

mod app;
mod log;
mod microphone;
mod room;
mod site;
mod speakers;

use std::env;
use std::time::Duration;

use anyhow::{bail, Result};
use serde_json::json;
use tokio::runtime::Runtime;
use tokio::time;
use windows::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};

use site::{Reply, Site};


fn main() {
    log::start();
    let args: Vec<String> = env::args().collect();
    let ticket = value_after(&args, "--ticket");
    let client = value_after(&args, "--client-pid").and_then(|pid| pid.parse().ok());
    let Some(client) = client else {
        log!("expected --client-pid <pid>");
        return;
    };

    Runtime::new().unwrap().block_on(async {
        tokio::select! {
            result = voice(ticket) => {
                if let Err(error) = result {
                    log!("stopped: {error:#}");
                }
            }
            () = client_exit(client) => log!("client exited"),
        }
    });
}


async fn voice(ticket: Option<String>) -> Result<()> {
    let Some(ticket) = ticket else {
        return app::talk().await;
    };

    let site = Site::open(json!({ "ticket": ticket })).await?;
    talk(&site).await
}


async fn talk(site: &Site) -> Result<()> {
    loop {
        match site.wait_for_grant().await {
            Ok(Reply::Ok(grant)) => {
                if let Err(error) = room::join(site, grant).await {
                    log!("voice: {error:#}");
                }
            }
            Ok(Reply::Wait) => continue,
            Ok(Reply::Denied) => bail!("session rejected"),
            Err(error) => log!("token: {error:#}"),
        }
        time::sleep(Duration::from_secs(2)).await;
    }
}


async fn client_exit(pid: u32) {
    while client_running(pid) {
        time::sleep(Duration::from_secs(1)).await;
    }
}


fn client_running(pid: u32) -> bool {
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else { return false };
        let running = WaitForSingleObject(process, 0) == WAIT_TIMEOUT;
        CloseHandle(process).ok();
        running
    }
}


fn value_after(args: &[String], flag: &str) -> Option<String> {
    let i = args.iter().position(|arg| arg == flag)?;
    args.get(i + 1).cloned()
}
