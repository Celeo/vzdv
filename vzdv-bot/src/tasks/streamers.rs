use anyhow::{Result, anyhow, bail};
use regex::Regex;
use scraper::{Html, Selector};
use std::sync::LazyLock;
use vzdv::{GENERAL_HTTP_CLIENT, config::Config};

const CHROME_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36";
static FIELD_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\W([A-Z]{3,4})\W"#).unwrap());

/// Using a link to a channel like `https://www.youtube.com/@<name>/live`, determine
/// first if the channel is online, and if so, return the live video title. If the
/// channel is not online, `Ok(None)` is returned.
async fn youtube_check(link: &str) -> Result<Option<String>> {
    let resp = GENERAL_HTTP_CLIENT
        .get(link)
        .header("user-agent", CHROME_USER_AGENT)
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("Got error from YouTube channel URL");
    }
    let document = Html::parse_document(&resp.text().await?);
    let href = document
        .select(&Selector::parse(r#"link[rel="canonical"]"#).unwrap())
        .next()
        .map(|ele| ele.attr("href").unwrap_or_default())
        .ok_or_else(|| anyhow!("Could not find channel video ref"))?;
    if !href.contains("/watch?v=") {
        return Ok(None);
    }

    let resp = GENERAL_HTTP_CLIENT
        .get(href)
        .header("user-agent", CHROME_USER_AGENT)
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("Got error from YouTube stream URL");
    }
    let title = document
        .select(&Selector::parse("title").unwrap())
        .next()
        .map(|ele| {
            ele.first_child()
                .map(|child| child.value().as_text().unwrap())
                .unwrap()
        })
        .ok_or_else(|| anyhow!("Could not find document title"))?;

    Ok(Some(title.text.to_string()))
}

async fn twitch_check(_link: &str) -> Result<Option<String>> {
    todo!()
}

/// Detect which streamers are currently online and are flying
/// to or from a vZDV field.
pub async fn detect_presence(config: &Config) -> Result<Vec<String>> {
    let mut valid = Vec::new();
    for link in &config.discord.streamers {
        let title = if link.contains("youtube.com") {
            youtube_check(link).await?
        } else if link.contains("twitch.com") {
            todo!()
        } else {
            unreachable!("unknown link type");
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
                println!(">>> {code}");
                if config
                    .airports
                    .all
                    .iter()
                    .any(|airport| airport.code == code)
                {
                    valid.push(link.to_owned());
                }
            }
        }
    }
    Ok(valid)
}

#[cfg(test)]
mod tests {
    use vzdv::config::Airport;

    use super::detect_presence;
    use crate::Config;

    #[tokio::test]
    async fn test_youtube_check() {
        let mut config = Config::default();
        let mut airport = Airport::default();
        airport.code = String::from("KDEN");
        config.airports.all.push(airport);
        config.discord.streamers = vec![String::from("")];

        //
    }
}
