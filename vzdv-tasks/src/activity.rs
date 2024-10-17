//! Update activity from VATSIM.

use anyhow::{Context, Result};
use chrono::Months;
use log::{debug, error};
use sqlx::{Pool, Row, Sqlite};
use std::{collections::HashMap, time::Duration};
use tokio::time;
use vatsim_utils::rest_api;
use vzdv::{
    config::Config,
    position_in_facility_airspace,
    sql::{self},
};

/// Update the activity for a single controller, looking back several
/// months and replacing the DB records with new data from VATSIM.
async fn true_up_single_activity(
    config: &Config,
    db: &Pool<Sqlite>,
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

    // transaction for these queries
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
pub async fn true_up_all_controllers_activity(config: &Config, db: &Pool<Sqlite>) -> Result<()> {
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
        let cid: u32 = row.try_get("cid").expect("no 'cid' column");
        debug!("Getting activity for {cid}");
        if let Err(e) = true_up_single_activity(config, db, &five_months_ago, cid).await {
            error!("Error updating activity for {cid}: {e}");
        }
        // wait a second to be nice to the VATSIM API
        time::sleep(Duration::from_secs(1)).await;
    }
    Ok(())
}

/// Updates a single controller's activity just this month.
async fn update_single_activity(
    config: &Config,
    db: &Pool<Sqlite>,
    start_of_month: &str,
    cid: u64,
) -> Result<()> {
    let sessions = rest_api::get_atc_sessions(cid, None, None, None, Some(start_of_month)).await?;
    let mut counter = 0.0;
    for session in sessions.results {
        if !position_in_facility_airspace(config, &session.callsign) {
            continue;
        }
        counter += session.minutes_on_callsign.parse::<f32>().unwrap() * 60.0;
    }
    sqlx::query(sql::UPDATE_ACTIVITY)
        .bind(cid as u32)
        .bind(start_of_month[0..7].to_string())
        .bind(counter)
        .execute(db)
        .await
        .with_context(|| format!("Updating CID {cid}"))?;
    Ok(())
}

/// Update this month's activity for currently online controllers.
pub async fn update_online_controller_activity(config: &Config, db: &Pool<Sqlite>) -> Result<()> {
    let on_roster_cids: Vec<u64> = {
        let rows = sqlx::query(sql::GET_ALL_ROSTER_CONTROLLER_CIDS)
            .fetch_all(db)
            .await?;
        rows.iter()
            .map(|row| row.try_get("cid").expect("no 'cid' column"))
            .collect()
    };
    let online_controllers = {
        let vatsim = vatsim_utils::live_api::Vatsim::new().await?;
        vatsim.get_v3_data().await?.controllers
    };
    let start_of_month = chrono::Utc::now()
        .checked_sub_months(Months::new(1))
        .unwrap()
        .format("%Y-%m-%d")
        .to_string();

    for controller in online_controllers {
        let cid = controller.cid;
        if !on_roster_cids.contains(&cid) {
            continue;
        }
        debug!("Spot-updating activity for {cid}");
        if let Err(e) = update_single_activity(config, db, &start_of_month, cid).await {
            error!("Error spot-updating CID {cid}: {e}")
        }
        // wait a second to be nice to the VATSIM API
        time::sleep(Duration::from_secs(1)).await;
    }

    Ok(())
}
