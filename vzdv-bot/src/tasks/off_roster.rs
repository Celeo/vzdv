use anyhow::Result;
use log::{debug, error, info};
use serde::Deserialize;
use sqlx::{Pool, Sqlite};
use std::{collections::HashSet, fmt::Write, sync::Arc, time::Duration};
use tokio::time::sleep;
use twilight_http::Client;
use twilight_model::id::Id;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};
use vatsim_utils::live_api::Vatsim;
use vzdv::{
    GENERAL_HTTP_CLIENT,
    config::Config,
    position_in_facility_airspace,
    sql::{self, Controller},
};

#[derive(Deserialize)]
struct AceControllerEntry {
    cid: u64,
}

#[derive(Deserialize)]
struct AceControllers {
    data: Vec<AceControllerEntry>,
}

/// Query the VATUSA API for the list of ACE controller CIDs.
async fn get_ace_controllers() -> Result<Vec<u64>> {
    let data: AceControllers = GENERAL_HTTP_CLIENT
        .get("https://api.vatusa.net/user/roles/ZHQ/ACE")
        .send()
        .await?
        .json()
        .await?;
    Ok(data.data.iter().map(|ace| ace.cid).collect())
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
