use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use serde_json::{json, Value};
use tokio::time;

use crate::log;
use crate::site::Site;

const CLAIM_KEY: &str = "OctaneVoiceClaim";


pub async fn talk() -> Result<()> {
    let mut claim = String::new();
    loop {
        claim = next_claim(&claim).await;
        match Site::open(json!({ "claim": claim })).await {
            Ok(site) => {
                if let Err(error) = crate::talk(&site).await {
                    log!("{error:#}");
                }
            }
            Err(error) => log!("claim: {error:#}"),
        }
    }
}


async fn next_claim(used: &str) -> String {
    loop {
        if let Some(claim) = newest_claim().filter(|claim| claim != used) {
            return claim;
        }
        time::sleep(Duration::from_secs(1)).await;
    }
}


fn newest_claim() -> Option<String> {
    let dir = PathBuf::from(env::var_os("LOCALAPPDATA")?).join("LocalStorage");
    let mut claims = Vec::new();

    for entry in fs::read_dir(dir).ok()?.flatten() {
        if !entry.file_name().to_string_lossy().ends_with("_appStorage.json") {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else { continue };
        let Ok(text) = fs::read(entry.path()) else { continue };
        let Ok(storage) = serde_json::from_slice::<Value>(&text) else { continue };
        let Some(claim) = storage[CLAIM_KEY].as_str() else { continue };
        claims.push((modified, claim.to_string()));
    }

    let (_, claim) = claims.into_iter().max_by_key(|(modified, _)| *modified)?;
    Some(claim)
}
