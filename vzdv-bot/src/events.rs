use anyhow::Result;
use log::info;
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use twilight_gateway::Event;
use twilight_http::Client;
use vzdv::config::Config;

use crate::tasks::roles::process_single_member;

/// Handle gateway events.
pub async fn handler(
    raw_event: &Event,
    http: &Arc<Client>,
    config: &Arc<Config>,
    db: &Pool<Sqlite>,
) -> Result<()> {
    if let Event::MemberAdd(event) = raw_event {
        /*
         * When a new member joins the guild, try to locate a controller by that Discord user ID
         * and update their member without waiting for then next roles task trigger.
         */
        let was_found =
            process_single_member(&event.member, event.guild_id, config, db, http).await;
        info!(
            "New member with user ID {} entered the guild; was_found = {was_found}",
            event.user.id
        );
    }
    Ok(())
}
