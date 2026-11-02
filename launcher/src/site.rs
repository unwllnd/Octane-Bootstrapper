use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use reqwest::blocking::Client;
use reqwest::header::RANGE;
use reqwest::StatusCode;

use crate::tls;

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) OctaneBootstrapper/0.1 (+https://www.octane.wtf)";
const ATTEMPTS: u32 = 4;
const RETRY_DELAY: Duration = Duration::from_secs(2);


pub fn client(timeout: Duration) -> Result<Client> {
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .timeout(timeout)
        .use_preconfigured_tls(tls::config()?)
        .build()?;
    Ok(client)
}


pub fn text(http: &Client, url: &str) -> Result<String> {
    let bytes = bytes(http, url)?;
    Ok(String::from_utf8(bytes)?.trim().to_string())
}


pub fn bytes(http: &Client, url: &str) -> Result<Vec<u8>> {
    for attempt in 1..ATTEMPTS {
        if let Ok(bytes) = get(http, url) {
            return Ok(bytes);
        }
        thread::sleep(RETRY_DELAY * attempt);
    }
    get(http, url)
}


pub fn download(http: &Client, url: &str, file: &Path, progress: &dyn Fn(f32)) -> Result<()> {
    let mut stalls = 0;
    loop {
        let before = saved(file);
        let Err(error) = resume(http, url, file, progress) else {
            return Ok(());
        };

        if saved(file) > before {
            stalls = 0;
        }
        stalls += 1;
        if stalls == ATTEMPTS {
            return Err(error);
        }
        thread::sleep(RETRY_DELAY * stalls);
    }
}


fn get(http: &Client, url: &str) -> Result<Vec<u8>> {
    let response = http.get(url).send()?.error_for_status()?;
    Ok(response.bytes()?.to_vec())
}


fn resume(http: &Client, url: &str, file: &Path, progress: &dyn Fn(f32)) -> Result<()> {
    let saved = saved(file);
    let response = http.get(url).header(RANGE, format!("bytes={saved}-")).send()?;
    if response.status() == StatusCode::RANGE_NOT_SATISFIABLE {
        return Ok(());
    }

    let mut response = response.error_for_status()?;
    let resumed = response.status() == StatusCode::PARTIAL_CONTENT;
    let mut done = if resumed { saved } else { 0 };
    let total = response.content_length().map(|left| done + left);
    let mut out = if resumed {
        OpenOptions::new().create(true).append(true).open(file)?
    } else {
        File::create(file)?
    };

    let mut chunk = vec![0; 64 * 1024];
    loop {
        let read = response.read(&mut chunk)?;
        if read == 0 {
            return Ok(());
        }

        out.write_all(&chunk[..read])?;
        done += read as u64;
        if let Some(total) = total {
            progress(done as f32 / total as f32);
        }
    }
}


fn saved(file: &Path) -> u64 {
    fs::metadata(file).map(|meta| meta.len()).unwrap_or(0)
}
