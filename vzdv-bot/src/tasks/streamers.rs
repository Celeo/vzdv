use anyhow::{Result, anyhow, bail};
use itertools::Itertools;
use log::{debug, error, info};
use regex::Regex;
use scraper::{Html, Selector};
use sqlx::{Pool, Sqlite};
use std::{sync::Arc, sync::LazyLock, time::Duration};
use tokio::time::sleep;
use twilight_http::Client;
use vzdv::{GENERAL_HTTP_CLIENT, config::Config};

const CHROME_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36";
static FIELD_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\W([A-Z]{3,4})\W"#).unwrap());

fn parse_for_live_link(text: &str) -> Result<String> {
    let document = Html::parse_document(text);
    let href = document
        .select(&Selector::parse(r#"link[rel="canonical"]"#).unwrap())
        .next()
        .map(|ele| ele.attr("href").unwrap_or_default())
        .ok_or_else(|| anyhow!("Could not find channel video ref"))?;
    Ok(href.to_owned())
}

fn parse_for_title(text: &str) -> Result<String> {
    let document = Html::parse_document(text);
    let title = document
        .select(&Selector::parse("title").unwrap())
        .next()
        .map(|ele| {
            ele.first_child()
                .map(|child| child.value().as_text().unwrap())
                .unwrap()
        })
        .ok_or_else(|| anyhow!("Could not find document title"))?;

    Ok(title.text.to_string())
}

/// Using a YouTube channel name, determine first if the channel is online, and if
/// so, return the live video title. If the channel is not online, `Ok(None)` is returned.
async fn youtube_check(channel: &str) -> Result<Option<String>> {
    // get link to the live video
    let resp = GENERAL_HTTP_CLIENT
        .get(format!("https://www.youtube.com/@{channel}/live"))
        .header("user-agent", CHROME_USER_AGENT)
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "Got error {} from YouTube channel lookup for {channel}",
            resp.status().as_u16()
        );
    }
    let href = parse_for_live_link(&resp.text().await?)?;
    if !href.contains("/watch?v=") {
        return Ok(None);
    }

    // get title of the live video
    let resp = GENERAL_HTTP_CLIENT
        .get(href)
        .header("user-agent", CHROME_USER_AGENT)
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "Got error {} from YouTube live video URL for {channel}",
            resp.status().as_u16()
        );
    }
    let title = parse_for_title(&resp.text().await?)?;
    Ok(Some(title.to_string()))
}

/// Using DecAPI, get the title of the current stream, if online, or `Ok(None)`.
async fn twitch_check(channel: &str) -> Result<Option<String>> {
    let resp = GENERAL_HTTP_CLIENT
        .get(format!("https://decapi.me/twitch/status/{channel}"))
        .header("user-agent", CHROME_USER_AGENT)
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "Got status {} from decapi for {channel}",
            resp.status().as_u16()
        );
    }
    let text = resp.text().await?;
    if text == format!("@{channel}") {
        return Ok(None);
    }
    Ok(Some(text.to_owned()))
}

/// Detect which streamers are currently online and are flying
/// to or from a vZDV field.
pub async fn detect_presence(config: &Config) -> Result<Vec<(String, Vec<&str>)>> {
    let mut valid = Vec::new();
    for part in &config.discord.streamers {
        let mut parts = part.split(':');
        let platform = parts
            .next()
            .ok_or_else(|| anyhow!("Invalid streamer config entry"))?;
        let name = parts
            .next()
            .ok_or_else(|| anyhow!("Invalid streamer config entry"))?;

        let title = match platform {
            "youtube" => youtube_check(name).await?,
            "twitch" => twitch_check(name).await?,
            _ => bail!("invalid streamer platform {platform}"),
        };

        if let Some(title) = title {
            for group in FIELD_REGEX.captures_iter(&title) {
                let code = {
                    let s = group.get(1).map(|s| s.as_str()).unwrap_or_default();
                    if s.len() == 3 {
                        format!("K{s}")
                    } else {
                        s.to_string()
                    }
                };
                let matching_airports: Vec<_> = config
                    .airports
                    .all
                    .iter()
                    .filter(|airport| airport.code == code)
                    .map(|airport| airport.code.as_str())
                    .collect();
                if !matching_airports.is_empty() {
                    valid.push((name.to_owned(), matching_airports));
                }
            }
        }
    }
    Ok(valid)
}

/// Single loop execution.
async fn tick(config: &Arc<Config>, _http: &Arc<Client>) -> Result<()> {
    debug!("Checking for online streamers");
    let streamers = detect_presence(config).await?;

    if !streamers.is_empty() {
        info!(
            "These streamers are active in relevant fields: {}",
            streamers.iter().map(|(name, _)| name).join(", ")
        );
    }

    Ok(())
}

/// Processing loop.
pub async fn process(config: Arc<Config>, _db: Pool<Sqlite>, http: Arc<Client>) {
    sleep(Duration::from_secs(30)).await;
    debug!("Starting streamers processing");

    loop {
        if let Err(e) = tick(&config, &http).await {
            error!("Error in streamers processing tick: {e}");
        }
        sleep(Duration::from_secs(60 * 15)).await; // 15 minutes
    }
}
