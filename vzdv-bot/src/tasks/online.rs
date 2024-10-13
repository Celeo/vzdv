use anyhow::Result;
use chrono::Utc;
use log::{debug, error};
use sqlx::{Pool, Sqlite};
use std::{fmt::Write, sync::Arc, time::Duration};
use tokio::time::sleep;
use twilight_http::Client;
use twilight_model::{channel::message::Embed, id::Id};
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder, EmbedFooterBuilder};
use vzdv::{config::Config, vatsim::get_online_facility_controllers};

async fn create_message(config: &Arc<Config>, db: &Pool<Sqlite>) -> Result<Embed> {
    let data = get_online_facility_controllers(db, config).await?;
    let enroute = {
        let s = data
            .iter()
            .filter(|c| {
                c.callsign.ends_with("_CTR")
                    || c.callsign.ends_with("_FSS")
                    || c.callsign.ends_with("_TMU")
            })
            .fold(String::new(), |mut acc, c| {
                writeln!(acc, "{} - {} - {}", c.callsign, c.name, c.online_for).unwrap();
                acc
            });
        if s.is_empty() {
            String::from("No controllers online")
        } else {
            s
        }
    };
    let tracon = {
        let s = data
            .iter()
            .filter(|c| c.callsign.ends_with("_APP") || c.callsign.ends_with("_DEP"))
            .fold(String::new(), |mut acc, c| {
                writeln!(acc, "{} - {} - {}", c.callsign, c.name, c.online_for).unwrap();
                acc
            });
        if s.is_empty() {
            String::from("No controllers online")
        } else {
            s
        }
    };
    let cab = {
        let s = data
            .iter()
            .filter(|c| {
                c.callsign.ends_with("_TWR")
                    || c.callsign.ends_with("_GND")
                    || c.callsign.ends_with("_DEL")
            })
            .fold(String::new(), |mut acc, c| {
                writeln!(acc, "{} - {} - {}", c.callsign, c.name, c.online_for).unwrap();
                acc
            });
        if s.is_empty() {
            String::from("No controllers online")
        } else {
            s
        }
    };

    let now = Utc::now();
    let embed = EmbedBuilder::new()
        .title("Online Controllers")
        .field(EmbedFieldBuilder::new("Enroute", enroute))
        .field(EmbedFieldBuilder::new("TRACON", tracon))
        .field(EmbedFieldBuilder::new("CAB", cab))
        .footer(EmbedFooterBuilder::new(format!(
            "Last updated: {} Denver, {} UTC",
            now.with_timezone(&chrono_tz::America::Denver)
                .format("%H:%M:%S"),
            now.format("%H:%M:%S"),
        )))
        .validate()?
        .build();
    Ok(embed)
}

/// Single loop execution.
async fn tick(config: &Arc<Config>, db: &Pool<Sqlite>, http: &Arc<Client>) -> Result<()> {
    let channel_id = Id::new(config.discord.online_channel);
    match config.discord.online_message {
        Some(id) => {
            http.update_message(channel_id, Id::new(id))
                .embeds(Some(&[create_message(config, db).await?]))?
                .await?;
        }
        None => {
            let resp = http
                .create_message(channel_id)
                .embeds(&[create_message(config, db).await?])?
                .await?
                .model()
                .await?;
            debug!("New online message ID: {}", resp.id.get());
        }
    }
    Ok(())
}

// Processing loop.
pub async fn process(config: Arc<Config>, db: Pool<Sqlite>, http: Arc<Client>) {
    sleep(Duration::from_secs(30)).await;
    debug!("Starting online processing");

    loop {
        if let Err(e) = tick(&config, &db, &http).await {
            error!("Error in online processing tick: {e}");
        }
        sleep(Duration::from_secs(60)).await; // 1 minute
    }
}
