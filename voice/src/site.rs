use std::time::Duration;

use anyhow::{bail, Result};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::time;

use crate::log;

const SITE: &str = "https://octane.wtf";

#[derive(Clone)]
pub struct Site {
    http: Client,
    session: String,
}

#[derive(Deserialize)]
struct Session {
    #[serde(rename = "session")]
    id: String,
    #[serde(rename = "userId")]
    user_id: String,
}

#[derive(Deserialize)]
pub struct Grant {
    pub url: String,
    pub token: String,
    pub room: String,
    pub identity: String,
    pub speaker: bool,
}

pub enum Reply<T> {
    Ok(T),
    Wait,
    Denied,
}


impl Site {
    pub async fn open(ticket: &str) -> Result<Site> {
        let http = Client::builder().timeout(Duration::from_secs(40)).build()?;
        loop {
            match post::<Session>(&http, "/voice-api/session", json!({ "ticket": ticket })).await {
                Ok(Reply::Ok(session)) => {
                    log!("session for user {}", session.user_id);
                    return Ok(Site { http, session: session.id });
                }
                Ok(Reply::Denied) => bail!("ticket rejected"),
                Ok(Reply::Wait) => {}
                Err(error) => log!("session: {error:#}"),
            }
            time::sleep(Duration::from_secs(3)).await;
        }
    }


    pub async fn wait_for_grant(&self) -> Result<Reply<Grant>> {
        let body = json!({ "session": self.session, "wait": true });
        post(&self.http, "/voice-api/token", body).await
    }


    pub async fn check_grant(&self) -> Result<Reply<Grant>> {
        let body = json!({ "session": self.session });
        post(&self.http, "/voice-api/token", body).await
    }


    pub fn report_speaking(&self, speaking: bool) {
        let site = self.clone();
        tokio::spawn(async move {
            let body = json!({ "session": site.session, "speaking": speaking });
            post::<Value>(&site.http, "/voice-api/speaking", body).await.ok();
        });
    }
}


async fn post<T: DeserializeOwned>(http: &Client, path: &str, body: Value) -> Result<Reply<T>> {
    let response = http.post(format!("{SITE}{path}")).json(&body).send().await?;
    match response.status().as_u16() {
        200 => {}
        403 => return Ok(Reply::Denied),
        status => bail!("{path} returned {status}"),
    }

    let answer: Value = response.json().await?;
    if answer["wait"] == true {
        return Ok(Reply::Wait);
    }
    Ok(Reply::Ok(serde_json::from_value(answer)?))
}
