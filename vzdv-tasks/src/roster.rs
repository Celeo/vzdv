//! Background task for updating the roster from VATUSA.

use anyhow::Result;
use chrono::DateTime;
use itertools::Itertools;
use log::{debug, error, info};
use sqlx::{Pool, Row, Sqlite, sqlite::SqliteRow};
use std::collections::HashSet;
use vzdv::{
    generate_operating_initials_for, retrieve_all_in_use_ois,
    sql::{self, Certification, Controller, IPC},
    vatusa::{self, MembershipType, RosterMember},
};

/// Update a single controller's stored data.
async fn update_controller_record(db: &Pool<Sqlite>, controller: &RosterMember) -> Result<()> {
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
    if controller_record.is_none() || !controller_record.unwrap().is_on_roster {
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
pub async fn update_roster(db: &Pool<Sqlite>) -> Result<()> {
    /*
     * Don't use a transaction here; instead, attempt to update every controller's
     * data. Don't error-out unless VATSIM doesn't give any data.
     */
    let roster_data = vatusa::get_roster("ZDV", MembershipType::Both).await?;
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
    let mut not_on_roster = Vec::new();
    for row in db_controllers {
        let cid: u32 = row.try_get("cid")?;
        if !current_controllers.contains(&cid) {
            not_on_roster.push(cid);
            if let Err(e) = sqlx::query(sql::UPDATE_REMOVED_FROM_ROSTER)
                .bind(cid)
                .execute(db)
                .await
            {
                error!("Error updating controller {cid} to show off-roster: {e}")
            }
            // strip certs
            let certs: Vec<Certification> = sqlx::query_as(sql::GET_ALL_CERTIFICATIONS_FOR)
                .bind(cid)
                .fetch_all(db)
                .await?;
            if certs.is_empty() {
                info!(
                    "Controller {cid} has certs for: {}",
                    certs
                        .iter()
                        .filter(|c| &c.value == "certified")
                        .map(|c| &c.name)
                        .join(",")
                );
                if let Err(e) = sqlx::query(sql::DELETE_CERTIFICATIONS_FOR)
                    .bind(cid)
                    .execute(db)
                    .await
                {
                    error!("Error removing certs for removed controller {cid}: {e}");
                }
            }
        }
    }
    debug!(
        "The following controllers are in the DB but not on the roster: {}",
        not_on_roster.iter().map(|n| n.to_string()).join(", ")
    );

    Ok(())
}

async fn partial_update_roster_single(db: &Pool<Sqlite>, message: &IPC) -> Result<()> {
    // update just this member in the roster
    let cid = message.data.parse()?;
    let controller = vatusa::get_controller_info(cid, None).await?;
    update_controller_record(db, &controller).await?;
    info!("Quick-updated roster for {cid}");
    Ok(())
}

/// Partially update the roster, just for CIDs that have been requested
/// by users of the site for a quick update.
pub async fn partial_update_roster(db: &Pool<Sqlite>) -> Result<()> {
    let ipc_messages: Vec<IPC> = sqlx::query_as(sql::GET_IPC_MESSAGES).fetch_all(db).await?;
    for message in ipc_messages {
        if message.action == "VATUSA_SYNC" {
            if let Err(e) = partial_update_roster_single(db, &message).await {
                error!("Error partially updating roster for a controller: {e}");
            }
            // delete the IPC message regardless of success or failure
            if let Err(e) = sqlx::query(sql::DELETE_IPC_MESSAGE)
                .bind(&message.uuid)
                .execute(db)
                .await
            {
                error!("Could not delete IPC message {}: {e}", message.uuid);
            }
        }
    }
    Ok(())
}
