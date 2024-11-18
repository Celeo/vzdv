use anyhow::Result;
use log::{debug, error, info};
use sqlx::{Pool, Sqlite};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::time::sleep;
use twilight_http::Client;
use twilight_model::id::Id;
use vzdv::{
    config::Config,
    get_controller_cids_and_names,
    sql::{self, Controller, NoShow},
};

/// Create the message to send to the Discord user.
fn create_message(
    no_show: &NoShow,
    cid_name_map: &HashMap<u32, (String, String)>,
    config: &Arc<Config>,
) -> String {
    format!(
        "## Warning\n\nYou have been added to the **{} no-show list** by {} {}.\n\nFor more information, reach out to the {} at `{}@{}`.\n\n\nResponses to this DM are not monitored.",
        no_show.entry_type,
        cid_name_map.get(&no_show.reported_by).unwrap().0,
        cid_name_map.get(&no_show.reported_by).unwrap().1,
        if no_show.entry_type == "training" { "TA" } else  { "EC" },
        if no_show.entry_type == "training" { "ta" } else  { "ec" },
        config.staff.email_domain
    )
}

/// Single loop execution.
async fn tick(config: &Arc<Config>, db: &Pool<Sqlite>, http: &Arc<Client>) -> Result<()> {
    debug!("Checking for new no-show entries");

    let entries: Vec<NoShow> = sqlx::query_as(sql::GET_ALL_NO_SHOW).fetch_all(db).await?;
    if entries.is_empty() {
        return Ok(());
    }
    let cid_name_map = get_controller_cids_and_names(db).await?;
    for entry in entries {
        if !entry.notified {
            debug!("Need to notify {} of no-show", entry.cid);
            let controller: Controller = sqlx::query_as(sql::GET_CONTROLLER_BY_CID)
                .bind(entry.cid)
                .fetch_one(db)
                .await?;
            if let Some(ref discord_user_id) = controller.discord_id {
                let channel = http
                    .create_private_channel(Id::new(discord_user_id.parse()?))
                    .await?
                    .model()
                    .await?;
                http.create_message(channel.id)
                    .content(&create_message(&entry, &cid_name_map, config))?
                    .await?;
                sqlx::query(sql::UPDATE_NO_SHOW_NOTIFIED)
                    .bind(entry.id)
                    .execute(db)
                    .await?;
                info!("Notified {} of their new no-show entry", entry.cid);
            } else {
                debug!(
                    "Controller {} does not have their Discord linked; cannot notify",
                    entry.cid
                );
            }
        }
    }

    Ok(())
}

// Processing loop.
pub async fn process(config: Arc<Config>, db: Pool<Sqlite>, http: Arc<Client>) {
    sleep(Duration::from_secs(30)).await;
    debug!("Starting no-show processing");

    loop {
        if let Err(e) = tick(&config, &db, &http).await {
            error!("Error in no-show processing tick: {e}");
        }
        sleep(Duration::from_secs(60 * 10)).await; // 10 minutes
    }
}
