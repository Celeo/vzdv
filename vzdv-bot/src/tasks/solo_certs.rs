use anyhow::Result;
use chrono::Utc;
use log::{debug, error, info};
use sqlx::{Pool, Sqlite};
use std::{sync::Arc, time::Duration};
use tokio::time::sleep;
use twilight_http::Client;
use twilight_model::id::Id;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};
use vzdv::{
    config::Config,
    get_controller_cids_and_names,
    sql::{self, SoloCert},
};

/// Single loop execution.
async fn tick(config: &Arc<Config>, db: &Pool<Sqlite>, http: &Arc<Client>) -> Result<()> {
    debug!("Checking for expiring solo certs");
    let solo_certs: Vec<SoloCert> = sqlx::query_as(sql::GET_ALL_SOLO_CERTS)
        .fetch_all(db)
        .await?;
    if solo_certs.is_empty() {
        return Ok(());
    }

    let cids_and_names = get_controller_cids_and_names(db).await?;
    let now = Utc::now();
    for cert in solo_certs {
        let delta = cert.expiration_date - now;
        if delta.num_hours() <= 12 && delta.num_hours() > 0 {
            info!("Expiration alert on solo cert #{}", cert.id);
            let name = cids_and_names
                .get(&cert.cid)
                .map(|n| format!("{} {}", n.0, n.1))
                .unwrap_or_else(|| String::from("? ?"));
            http.create_message(Id::new(config.discord.solo_cert_expiration_channel))
                .embeds(&[EmbedBuilder::new()
                    .title("Expiring solo cert")
                    .field(EmbedFieldBuilder::new(
                        "Controller",
                        format!("{} ({})", name, cert.cid),
                    ))
                    .field(EmbedFieldBuilder::new("Position", cert.position))
                    .field(EmbedFieldBuilder::new(
                        "Expires",
                        cert.expiration_date.to_rfc2822(),
                    ))
                    .validate()?
                    .build()])?
                .await?;
            debug!("Expiring solo cert message posted to Discord");
        }
    }

    Ok(())
}

// Processing loop.
pub async fn process(config: Arc<Config>, db: Pool<Sqlite>, http: Arc<Client>) {
    sleep(Duration::from_secs(30)).await;
    debug!("Starting solo cert processing");

    loop {
        if let Err(e) = tick(&config, &db, &http).await {
            error!("Error in solo cert processing tick: {e}");
        }
        sleep(Duration::from_secs(60 * 60 * 12)).await; // 12 hours
    }
}
