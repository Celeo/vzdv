//! vZDV importer to get data from existing site.

#![deny(clippy::all)]
#![deny(unsafe_code)]

use anyhow::{bail, Result};
use clap::Parser;
use log::{debug, error, info, warn};
use serde::Deserialize;
use sqlx::{Pool, Sqlite};
use std::{collections::HashMap, path::PathBuf};
use vzdv::{general_setup, ControllerRating, GENERAL_HTTP_CLIENT};

const ROSTER_URL: &str = "https://api.zdvartcc.org/v1/user/all";

/// vZDV importer to get data from existing site.
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Load the config from a specific file.
    ///
    /// [default: vzdv.toml]
    #[arg(long)]
    config: Option<PathBuf>,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,
}

#[derive(Deserialize)]
struct AdhCertification {
    display_name: String,
    value: String,
}

#[derive(Deserialize)]
struct AdhController {
    cid: u32,
    first_name: String,
    last_name: String,
    operating_initials: String,
    certifications: HashMap<String, AdhCertification>,
    rating: String,
    discord_id: String,
}

async fn get_adh_data() -> Result<Vec<AdhController>> {
    let response = GENERAL_HTTP_CLIENT.get(ROSTER_URL).send().await?;
    if !response.status().is_success() {
        bail!(
            "Got status {} from ZDV ADH roster endpoint",
            response.status().as_u16()
        );
    }
    let data: Vec<AdhController> = response.json().await?;
    info!("Got {} controllers from the ZDV roster", data.len());
    Ok(data)
}

async fn update_single(db: &Pool<Sqlite>, controller: &AdhController) -> Result<()> {
    debug!("Updating {}", controller.cid);

    let discord_id: Option<String> = if controller.discord_id.is_empty() {
        None
    } else {
        Some(controller.discord_id.clone())
    };
    let rows =
        sqlx::query("UPDATE controller SET operating_initials=$1, discord_id=$2 WHERE cid=$3")
            .bind(controller.operating_initials.clone())
            .bind(&discord_id)
            .bind(controller.cid)
            .execute(db)
            .await?;

    if rows.rows_affected() == 0 {
        debug!("New controller");
        // unknown controller, very likely off-roster
        let sql = "INSERT INTO controller (id, cid, first_name, last_name, rating, is_on_roster, discord_id) VALUES (NULL, $1, $2, $3, $4, FALSE, $5)";
        let rating = match controller.rating.as_str() {
            "INA" => ControllerRating::INA,
            "SUS" => ControllerRating::SUS,
            "OBS" => ControllerRating::OBS,
            "S1" => ControllerRating::S1,
            "S2" => ControllerRating::S2,
            "S3" => ControllerRating::S3,
            "C1" => ControllerRating::C1,
            "C2" => ControllerRating::C2,
            "C3" => ControllerRating::C3,
            "I1" => ControllerRating::I1,
            "I2" => ControllerRating::I2,
            "I3" => ControllerRating::I3,
            "SUP" => ControllerRating::SUP,
            "ADM" => ControllerRating::ADM,
            _ => {
                warn!("Unknown controller rating string: {}", &controller.rating);
                ControllerRating::OBS
            }
        };
        sqlx::query(sql)
            .bind(controller.cid)
            .bind(&controller.first_name)
            .bind(&controller.last_name)
            .bind(rating.as_id())
            .bind(discord_id)
            .execute(db)
            .await?;
    } else {
        debug!("Existing controller");
    }

    sqlx::query("DELETE FROM certification WHERE cid=$1")
        .bind(controller.cid)
        .execute(db)
        .await?;
    for certification in controller.certifications.values() {
        if certification.value == "none" {
            continue;
        }
        sqlx::query("INSERT INTO certification (id, cid, name, value, changed_on, set_by) VALUES (NULL, $1, $2, $3, $4, $5)")
            .bind(controller.cid)
            .bind(&certification.display_name)
            .bind(&certification.value)
            .bind(chrono::Utc::now())
            .bind(0)
            .execute(db)
            .await?;
    }

    Ok(())
}

/// Entrypoint.
#[allow(clippy::needless_return)] // https://github.com/rust-lang/rust-clippy/issues/13458
#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let (_config, db) = general_setup(cli.debug, "vzdv_import", cli.config).await;

    info!("Retrieving data");
    let data = match get_adh_data().await {
        Ok(d) => d,
        Err(e) => {
            error!("Error getting data: {e}");
            return;
        }
    };

    info!("Saving changes");
    for controller in data {
        if let Err(e) = update_single(&db, &controller).await {
            error!("Error updating controller {}: {e}", controller.cid);
        }
    }

    info!("Complete");
}
