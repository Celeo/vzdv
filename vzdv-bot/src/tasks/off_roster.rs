use anyhow::{anyhow, Result};
use log::{debug, error, info};
use select::{document::Document, predicate::Name};
use sqlx::{Pool, Sqlite};
use std::{collections::HashSet, fmt::Write, sync::Arc, time::Duration};
use tokio::time::sleep;
use twilight_http::Client;
use twilight_model::id::Id;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};
use vatsim_utils::live_api::Vatsim;
use vzdv::{
    config::Config,
    position_in_facility_airspace,
    sql::{self, Controller},
    GENERAL_HTTP_CLIENT,
};

/// Query the VATUSA website for the list of ACE controllers' CIDs.
async fn get_ace_controllers() -> Result<Vec<u64>> {
    let html = GENERAL_HTTP_CLIENT
        .get("https://www.vatusa.net/info/ace")
        .send()
        .await?
        .text()
        .await?;

    let doc = Document::from(html.as_str());
    let table_body = doc
        .find(Name("table"))
        .next()
        .ok_or_else(|| anyhow!("Empty iter"))?
        .find(Name("tbody"))
        .next()
        .ok_or_else(|| anyhow!("Empty iter"))?;

    let cids = table_body
        .find(Name("tr"))
        .flat_map(|row| match row.find(Name("td")).next() {
            Some(col) => {
                let text = col.text();
                match text.parse() {
                    Ok(n) => Some(n),
                    Err(_) => None,
                }
            }
            None => None,
        })
        .collect();
    Ok(cids)
}

/// Single loop execution.
async fn tick(config: &Arc<Config>, db: &Pool<Sqlite>, http: &Arc<Client>) -> Result<()> {
    let data = Vatsim::new().await?.get_v3_data().await?;
    let on_roster: Vec<Controller> = sqlx::query_as(sql::GET_ALL_CONTROLLERS_ON_ROSTER)
        .fetch_all(db)
        .await?;
    let on_roster_cids = {
        let mut set: HashSet<u64> = on_roster.iter().map(|c| c.cid as u64).collect();
        let ace = get_ace_controllers().await?;
        set.extend(ace.iter());
        set
    };

    let mut violations = String::new();
    for online in data.controllers {
        if position_in_facility_airspace(config, &online.callsign)
            && !on_roster_cids.contains(&online.cid)
        {
            let s = format!(
                "{} ({}) on {} is not on the roster",
                online.name, online.cid, online.callsign
            );
            info!("{s}");
            writeln!(violations, "{s}\n")?;
        }
    }

    if !violations.is_empty() {
        http.create_message(Id::new(config.discord.off_roster_channel))
            .embeds(&[EmbedBuilder::new()
                .title("Off-roster controllers")
                .field(EmbedFieldBuilder::new("", violations).inline())
                .validate()?
                .build()])?
            .await?;
        info!("Message posted to Discord");
    }

    Ok(())
}

// Processing loop.
pub async fn process(config: Arc<Config>, db: Pool<Sqlite>, http: Arc<Client>) {
    sleep(Duration::from_secs(30)).await;
    debug!("Starting off-roster controller processing");

    loop {
        if let Err(e) = tick(&config, &db, &http).await {
            error!("Error in off-roster controller processing tick: {e}");
        }
        sleep(Duration::from_secs(60 * 5)).await; // 5 minutes
    }
}
