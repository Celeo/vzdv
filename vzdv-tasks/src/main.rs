//! vZDV website background task runner.

#![deny(clippy::all)]
#![deny(unsafe_code)]

use anyhow::{Context, Result};
use chrono::{DateTime, Months};
use clap::Parser;
use log::{debug, error, info};
use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::Duration,
};
use tokio::time;
use vatsim_utils::rest_api;
use vzdv::{
    config::Config,
    general_setup, generate_operating_initials_for, position_in_facility_airspace,
    retrieve_all_in_use_ois,
    sql::{self, Controller},
    vatusa::{get_roster, MembershipType, RosterMember},
};

/// vZDV task runner.
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

/// Update a single controller's stored data.
async fn update_controller_record(db: &SqlitePool, controller: &RosterMember) -> Result<()> {
    // VATUSA doesn't handle Jr staff roles well, so ignore them in the sync, but do keep Mentors
    let roles_to_match = &["ATM", "DATM", "TA", "MTR"];
    let roles = {
        let mut roles: Vec<_> = controller
            .roles
            .iter()
            .filter(|role| role.facility == "ZDV")
            .flat_map(|role| {
                let n = &role.role;
                if roles_to_match.contains(&n.as_str()) {
                    Some(n.clone())
                } else {
                    None
                }
            })
            .collect();
        // add in home controllers with Instructor network rating
        if controller.facility == "ZDV" && [8, 9, 10].contains(&controller.rating) {
            roles.push(String::from("INS"));
        }
        roles
    };

    // pull the existing DB data
    let controller_record: Option<Controller> = sqlx::query_as(sql::GET_CONTROLLER_BY_CID)
        .bind(controller.cid)
        .fetch_optional(db)
        .await?;

    // merge any new roles with any existing roles
    let roles = match &controller_record {
        Some(cr) => {
            let mut all_roles = HashSet::new();
            cr.roles.split(',').for_each(|r| {
                all_roles.insert(r);
            });
            roles.iter().for_each(|r| {
                all_roles.insert(r);
            });
            all_roles.iter().map(|s| s.to_string()).collect()
        }
        None => roles,
    };

    // update main record
    sqlx::query(sql::UPSERT_USER_TASK)
        .bind(controller.cid)
        .bind(&controller.first_name)
        .bind(&controller.last_name)
        .bind(&controller.email)
        .bind(controller.rating)
        .bind(&controller.facility)
        // controller will be on the roster since that's what the VATSIM API is showing
        .bind(true)
        .bind(DateTime::parse_from_rfc3339(&controller.facility_join)?)
        .bind(roles.join(","))
        .execute(db)
        .await?;
    // for controllers new to the ARTCC, also set their default OIs
    if controller_record.is_none() {
        let in_use = retrieve_all_in_use_ois(db).await?;
        let new_ois = generate_operating_initials_for(
            &in_use,
            &controller.first_name,
            &controller.last_name,
        )?;
        sqlx::query(sql::UPDATE_CONTROLLER_OIS)
            .bind(controller.cid)
            .bind(&new_ois)
            .execute(db)
            .await?;
        info!(
            "{} {} ({}) added to DB with OIs {new_ois}",
            &controller.first_name, &controller.last_name, controller.cid
        );
    } else {
        debug!(
            "{} {} ({}) updated in DB",
            &controller.first_name, &controller.last_name, controller.cid
        );
    }
    Ok(())
}

/// Update the stored roster with fresh data from VATUSA.
async fn update_roster(db: &SqlitePool) -> Result<()> {
    /*
     * Don't use a transaction here; instead, attempt to update every controller's
     * data. Don't error-out unless VATSIM doesn't give any data.
     */
    let roster_data = get_roster("ZDV", MembershipType::Both).await?;
    debug!("Got roster response");
    for controller in &roster_data {
        if let Err(e) = update_controller_record(db, controller).await {
            error!("Error updating controller {} in DB: {e}", controller.cid);
        };
    }

    debug!("Checking for removed controllers");
    let current_controllers: Vec<_> = roster_data
        .iter()
        .map(|controller| controller.cid)
        .collect();
    let db_controllers: Vec<SqliteRow> = sqlx::query(sql::GET_ALL_CONTROLLER_CIDS)
        .fetch_all(db)
        .await?;
    for row in db_controllers {
        let cid: u32 = row.try_get("cid")?;
        if !current_controllers.contains(&cid) {
            debug!("Controller {cid} is not on the roster");
            if let Err(e) = sqlx::query(sql::UPDATE_REMOVED_FROM_ROSTER)
                .bind(cid)
                .execute(db)
                .await
            {
                error!("Error updating controller {cid} to show off-roster: {e}")
            }
        }
    }

    Ok(())
}

/// Update the activity for a single controller.
///
/// In a separate function to easily use the `?` operator.
async fn update_single_activity(
    config: &Config,
    db: &SqlitePool,
    five_months_ago: &str,
    cid: u32,
) -> Result<()> {
    /*
     * Get the last 5 months of the controller's activity.
     *
     * I'm not (currently) worried about pagination as even the facility's most
     * active controllers don't have enough sessions in this time range to go over
     * the endpoint's single-page response limit.
     */
    let sessions = rest_api::get_atc_sessions(cid as u64, None, None, Some(five_months_ago), None)
        .await
        .with_context(|| format!("Processing CID {cid}"))?;
    // group the controller's activity by month
    let mut seconds_map: HashMap<String, f32> = HashMap::new();
    for session in sessions.results {
        // filter to only sessions in the facility
        if !position_in_facility_airspace(config, &session.callsign) {
            continue;
        }

        let month = session.start[0..7].to_string();
        let seconds = session.minutes_on_callsign.parse::<f32>().unwrap() * 60.0;
        seconds_map
            .entry(month)
            .and_modify(|acc| *acc += seconds)
            .or_insert(seconds);
    }

    // transaction for the ~6 queries
    let mut tx = db.begin().await?;
    // clear the controller's existing records in prep for replacement
    sqlx::query(sql::DELETE_ACTIVITY_FOR_CID)
        .bind(cid)
        .execute(&mut *tx)
        .await
        .with_context(|| format!("Processing CID {cid}"))?;
    // for each relevant month, store their total controlled minutes in the DB
    for (month, seconds) in seconds_map {
        let minutes = (seconds / 60.0).round() as u32;
        sqlx::query(sql::INSERT_INTO_ACTIVITY)
            .bind(cid)
            .bind(month)
            .bind(minutes)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("Processing CID {cid}"))?;
    }
    // commit the controller's changes
    tx.commit().await?;

    Ok(())
}

/// Update all controllers' stored activity data with data from VATSIM.
///
/// For each controller in the DB, their activity data will be cleared,
/// and then (for on-roster controllers) fetched and stored in the DB as
/// part of a transaction.
async fn update_activity(config: &Config, db: &SqlitePool) -> Result<()> {
    // prep cids for on-roster controllers and a 5-month-ago timestamp that the API recognizes
    let controllers = sqlx::query(sql::GET_ALL_ROSTER_CONTROLLER_CIDS)
        .fetch_all(db)
        .await?;
    let five_months_ago = chrono::Utc::now()
        .checked_sub_months(Months::new(5))
        .unwrap()
        .format("%Y-%m-%d")
        .to_string();
    for row in controllers {
        let cid: u32 = row.try_get("cid")?;
        debug!("Getting activity for {cid}");
        if let Err(e) = update_single_activity(config, db, &five_months_ago, cid).await {
            error!("Error updating activity for {cid}: {e}");
        }
        // wait a second to be nice to the VATSIM API
        time::sleep(Duration::from_secs(1)).await;
    }
    Ok(())
}

/// Entrypoint.
#[allow(clippy::needless_return)] // https://github.com/rust-lang/rust-clippy/issues/13458
#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let (config, db) = general_setup(cli.debug, "vzdv_tasks", cli.config).await;

    info!("Starting tasks");
    let roster_handle = {
        let db = db.clone();
        tokio::spawn(async move {
            debug!("Waiting 10 seconds before starting roster sync");
            time::sleep(time::Duration::from_secs(10)).await;
            loop {
                info!("Querying roster");
                match update_roster(&db).await {
                    Ok(_) => {
                        info!("Roster update successful");
                    }
                    Err(e) => {
                        error!("Error updating roster: {e}");
                    }
                }
                debug!("Waiting 2 hours for next roster sync");
                time::sleep(time::Duration::from_secs(60 * 60 * 2)).await;
            }
        })
    };

    let activity_handle = {
        let config = config.clone();
        let db = db.clone();
        tokio::spawn(async move {
            debug!("Waiting 60 seconds before starting activity sync");
            time::sleep(time::Duration::from_secs(60)).await;
            loop {
                info!("Updating activity");
                match update_activity(&config, &db).await {
                    Ok(_) => {
                        info!("Activity update successful");
                    }
                    Err(e) => {
                        error!("Error updating activity: {e}");
                    }
                }
                debug!("Waiting 6 hours for next activity sync");
                time::sleep(time::Duration::from_secs(60 * 60 * 6)).await;
            }
        })
    };

    roster_handle.await.unwrap();
    activity_handle.await.unwrap();

    db.close().await;
}
