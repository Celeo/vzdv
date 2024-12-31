//! Helper functions for dealing with DokuWiki.
//!
//! Unfortunately, while HTTP API endpoints exist in DokuWiki for
//! creating and deleting users, no endpoints exist for checking
//! if a user exists or changing their groups.

use crate::{
    config::Config,
    sql::{self, Controller},
    GENERAL_HTTP_CLIENT,
};
use anyhow::{bail, Result};
use log::error;
use serde_json::json;
use sqlx::{Pool, Sqlite};
use tokio::fs;

/// Read the DokuWiki's users file.
async fn read_file(config: &Config) -> Result<Vec<String>> {
    let content = fs::read_to_string(&config.dokuwiki.user_file_path).await?;
    Ok(content
        .split_terminator('\n')
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_owned())
        .collect())
}

/// Returns true if the user exists in DokuWiki.
pub async fn check_user_exists(config: &Config, cid: u32) -> Result<bool> {
    let content = read_file(config).await?;
    let cid_str = format!("{cid}");
    let exists = content.iter().any(|line| line.starts_with(&cid_str));
    Ok(exists)
}

/// Use the DokuWiki HTTP API to create a user.
pub async fn create_user(config: &Config, cid: u32, db: &Pool<Sqlite>) -> Result<()> {
    let controller: Option<Controller> = sqlx::query_as(sql::GET_CONTROLLER_BY_CID)
        .bind(cid)
        .fetch_optional(db)
        .await?;
    let controller = match controller {
        Some(c) => c,
        None => {
            bail!("could not find controller for cid {cid}");
        }
    };

    let res = GENERAL_HTTP_CLIENT
        .post(format!(
            "{}lib/exe/jsonrpc.php/plugin.usermanager.createUser",
            config.dokuwiki.domain
        ))
        .header(
            reqwest::header::AUTHORIZATION,
            &format!("Bearer {}", config.dokuwiki.api_token),
        )
        .json(&json!({
            "user": format!("{cid}"),
            "name": format!("{} {}", &controller.first_name, &controller.last_name),
            "email": "blackhole@example.com",
            "groups": ["user"],
            "password": "",
            "notify": false,
        }))
        .send()
        .await?;
    if !res.status().is_success() {
        bail!(
            "got status {} from DokuWiki create API",
            res.status().as_u16()
        );
    }
    let data: serde_json::Value = res.json().await?;
    if !data.get("result").unwrap().as_bool().unwrap_or_default() {
        error!(
            "Response from DokuWiki create API: {}",
            serde_json::to_string(&data)?
        );
        bail!("DokuWiki account creation failed");
    }
    Ok(())
}
