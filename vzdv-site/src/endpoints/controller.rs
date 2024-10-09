//! HTTP endpoints for controller pages.

use crate::{
    flashed_messages::{self, MessageLevel},
    shared::{
        is_user_member_of, js_timestamp_to_utc, reject_if_not_in, AppError, AppState, UserInfo,
        SESSION_USER_INFO_KEY,
    },
};
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{delete, get, post},
    Form, Router,
};
use chrono::{DateTime, NaiveDateTime, Utc};
use itertools::Itertools;
use log::{error, info, warn};
use minijinja::{context, Environment};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tower_sessions::Session;
use vzdv::{
    controller_can_see, get_controller_cids_and_names, retrieve_all_in_use_ois,
    sql::{self, Certification, Controller, Feedback, StaffNote},
    vatusa::{
        get_multiple_controller_names, get_training_records, save_training_record,
        NewTrainingRecord, TrainingRecord,
    },
    ControllerRating, PermissionsGroup, StaffPosition,
};

/// Roles the current user is able to set.
async fn roles_to_set(
    db: &Pool<Sqlite>,
    user_info: &Option<UserInfo>,
) -> Result<HashSet<String>, AppError> {
    let controller: Option<Controller> = match user_info {
        Some(ref ui) => {
            sqlx::query_as(sql::GET_CONTROLLER_BY_CID)
                .bind(ui.cid)
                .fetch_optional(db)
                .await?
        }
        None => None,
    };
    let mut roles_to_set = Vec::new();
    let user_roles: Vec<_> = match &controller {
        Some(c) => c.roles.split_terminator('\n').collect(),
        None => {
            return Ok(HashSet::new());
        }
    };
    if user_roles.contains(&"FE") {
        roles_to_set.push(StaffPosition::AFE);
    } else if user_roles.contains(&"EC") {
        roles_to_set.push(StaffPosition::AEC);
    } else if controller_can_see(&controller, PermissionsGroup::Admin) {
        roles_to_set.push(vzdv::StaffPosition::ATM);
        roles_to_set.push(vzdv::StaffPosition::DATM);
        roles_to_set.push(vzdv::StaffPosition::TA);
        roles_to_set.push(vzdv::StaffPosition::FE);
        roles_to_set.push(vzdv::StaffPosition::EC);
        roles_to_set.push(vzdv::StaffPosition::WM);
        roles_to_set.push(vzdv::StaffPosition::AFE);
        roles_to_set.push(vzdv::StaffPosition::AEC);
        roles_to_set.push(vzdv::StaffPosition::AWM);
        roles_to_set.push(vzdv::StaffPosition::INS);
        roles_to_set.push(vzdv::StaffPosition::MTR);
    }

    Ok(roles_to_set
        .iter()
        .map(|position| position.as_str().to_owned())
        .collect::<HashSet<String>>())
}

/// Overview page for a user.
///
/// Shows additional information and controls for different staff
/// members (some, training, admin).
async fn page_controller(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(cid): Path<u32>,
) -> Result<Response, AppError> {
    #[derive(Serialize)]
    struct CertNameValue<'a> {
        name: &'a str,
        value: &'a str,
    }

    #[derive(Serialize)]
    struct StaffNoteDisplay {
        id: u32,
        by: String,
        by_cid: u32,
        date: DateTime<Utc>,
        comment: String,
    }

    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let controller: Option<Controller> = sqlx::query_as(sql::GET_CONTROLLER_BY_CID)
        .bind(cid)
        .fetch_optional(&state.db)
        .await?;
    let controller = match controller {
        Some(c) => c,
        None => {
            flashed_messages::push_flashed_message(
                session,
                flashed_messages::MessageLevel::Error,
                "Controller not found",
            )
            .await?;
            return Ok(Redirect::to("/facility/roster").into_response());
        }
    };
    let rating_str = ControllerRating::try_from(controller.rating)
        .map_err(|err| AppError::GenericFallback("parsing unknown controller rating", err))?
        .as_str();

    let db_certs: Vec<Certification> = sqlx::query_as(sql::GET_ALL_CERTIFICATIONS_FOR)
        .bind(cid)
        .fetch_all(&state.db)
        .await?;
    let mut certifications: Vec<CertNameValue> =
        Vec::with_capacity(state.config.training.certifications.len());
    let none = String::from("None");
    for name in &state.config.training.certifications {
        let db_match = db_certs.iter().find(|cert| &cert.name == name);
        let value: &str = match db_match {
            Some(row) => &row.value,
            None => &none,
        };
        certifications.push(CertNameValue { name, value });
    }
    let roles: Vec<_> = controller.roles.split_terminator(',').collect();

    let is_admin = is_user_member_of(&state, &user_info, PermissionsGroup::Admin).await;
    let feedback: Vec<Feedback> = if is_admin {
        sqlx::query_as(sql::GET_ALL_FEEDBACK_FOR)
            .bind(cid)
            .fetch_all(&state.db)
            .await?
    } else {
        Vec::new()
    };
    let staff_notes: Vec<StaffNoteDisplay> = if is_admin {
        let notes: Vec<StaffNote> = sqlx::query_as(sql::GET_STAFF_NOTES_FOR)
            .bind(cid)
            .fetch_all(&state.db)
            .await?;
        let controllers = get_controller_cids_and_names(&state.db)
            .await
            .map_err(|e| AppError::GenericFallback("getting names and CIDs from DB", e))?;
        notes
            .iter()
            .map(|note| StaffNoteDisplay {
                id: note.id,
                by: controllers
                    .iter()
                    .find(|c| *c.0 == note.by)
                    .map(|c| format!("{} {} ({})", c.1 .0, c.1 .1, c.0))
                    .unwrap_or_else(|| format!("{}?", note.cid)),
                by_cid: note.by,
                date: note.date,
                comment: note.comment.clone(),
            })
            .collect()
    } else {
        Vec::new()
    };
    let settable_roles_set = roles_to_set(&state.db, &user_info).await?;
    let mut settable_roles: Vec<_> = settable_roles_set.iter().collect();
    settable_roles.sort();

    let flashed_messages = flashed_messages::drain_flashed_messages(session).await?;
    let template = state.templates.get_template("controller/controller")?;
    let rendered: String = template.render(context! {
        user_info,
        controller,
        roles,
        rating_str,
        certifications,
        settable_roles,
        feedback,
        staff_notes,
        flashed_messages
    })?;
    Ok(Html(rendered).into_response())
}

/// API endpoint to unlink a controller's Discord account.
///
/// For admin staff members.
async fn api_unlink_discord(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(cid): Path<u32>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if let Some(redirect) = reject_if_not_in(&state, &user_info, PermissionsGroup::Admin).await {
        return Ok(redirect);
    }
    sqlx::query(sql::UNSET_CONTROLLER_DISCORD_ID)
        .bind(cid)
        .execute(&state.db)
        .await?;
    flashed_messages::push_flashed_message(session, MessageLevel::Info, "Discord unlinked").await?;
    info!(
        "{} unlinked Discord account from {cid}",
        user_info.unwrap().cid
    );
    Ok(Redirect::to(&format!("/controllers/{cid}")))
}

#[derive(Deserialize)]
struct ChangeInitialsForm {
    initials: String,
}

/// Form submission to set a controller's operating initials.
///
/// For admin staff members.
async fn post_change_ois(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(cid): Path<u32>,
    Form(initials_form): Form<ChangeInitialsForm>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if let Some(redirect) = reject_if_not_in(&state, &user_info, PermissionsGroup::Admin).await {
        return Ok(redirect);
    }
    let initials = initials_form.initials.to_uppercase();

    // assert unique
    if !initials.is_empty() {
        let in_use = retrieve_all_in_use_ois(&state.db)
            .await
            .map_err(|err| AppError::GenericFallback("accessing DB to get existing OIs", err))?;
        if in_use.contains(&initials) {
            flashed_messages::push_flashed_message(
                session,
                MessageLevel::Error,
                "Those OIs are already in use",
            )
            .await?;
            return Ok(Redirect::to(&format!("/controller/{cid}")));
        }
    }

    // update
    sqlx::query(sql::UPDATE_CONTROLLER_OIS)
        .bind(cid)
        .bind(&initials)
        .execute(&state.db)
        .await?;

    flashed_messages::push_flashed_message(
        session,
        MessageLevel::Info,
        "Operating initials updated",
    )
    .await?;
    info!(
        "{} updated OIs for {cid} to: '{initials}'",
        user_info.unwrap().cid,
    );
    Ok(Redirect::to(&format!("/controller/{cid}")))
}

/// Form submission to set the controller's certifications.
///
/// Not used to set their network rating; that process is handled
/// through VATUSA/VATSIM. Also does not handle communicating solo
/// certs to any other site.
///
/// For training staff members.
async fn post_change_certs(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(cid): Path<u32>,
    Form(certs_form): Form<HashMap<String, String>>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if let Some(redirect) =
        reject_if_not_in(&state, &user_info, PermissionsGroup::TrainingTeam).await
    {
        return Ok(redirect);
    }

    let by_cid = user_info.unwrap().cid;
    let db_certs: Vec<Certification> = sqlx::query_as(sql::GET_ALL_CERTIFICATIONS_FOR)
        .bind(cid)
        .fetch_all(&state.db)
        .await?;
    for (key, value) in &certs_form {
        let existing = db_certs.iter().find(|c| &c.name == key);
        match existing {
            Some(existing) => {
                sqlx::query(sql::UPDATE_CERTIFICATION)
                    .bind(existing.id)
                    .bind(value)
                    .bind(Utc::now())
                    .bind(by_cid)
                    .execute(&state.db)
                    .await?;
                info!("{by_cid} updated cert for {cid} of {key} -> {value}");
            }
            None => {
                sqlx::query(sql::CREATE_CERTIFICATION)
                    .bind(cid)
                    .bind(key)
                    .bind(value)
                    .bind(Utc::now())
                    .bind(by_cid)
                    .execute(&state.db)
                    .await?;
                info!("{by_cid} created new cert for {cid} of {key} -> {value}");
            }
        }
    }

    flashed_messages::push_flashed_message(session, MessageLevel::Info, "Updated certifications")
        .await?;
    Ok(Redirect::to(&format!("/controller/{cid}")))
}

#[derive(Deserialize)]
struct NewNoteForm {
    note: String,
}

/// Post a new staff note to the controller.
///
/// For staff members.
async fn post_new_staff_note(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(cid): Path<u32>,
    Form(note_form): Form<NewNoteForm>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if let Some(redirect) = reject_if_not_in(&state, &user_info, PermissionsGroup::SomeStaff).await
    {
        return Ok(redirect);
    }
    let user_info = user_info.unwrap();
    info!("{} added staff note to {cid}", user_info.cid);
    sqlx::query(sql::CREATE_STAFF_NOTE)
        .bind(cid)
        .bind(user_info.cid)
        .bind(Utc::now())
        .bind(note_form.note)
        .execute(&state.db)
        .await?;
    flashed_messages::push_flashed_message(session, MessageLevel::Info, "Message saved").await?;
    Ok(Redirect::to(&format!("/controller/{cid}")))
}

/// Delete a staff note. The user performing the deletion must be the user who left the note.
///
/// For staff members.
async fn api_delete_staff_note(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path((_cid, note_id)): Path<(u32, u32)>,
) -> Result<StatusCode, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if reject_if_not_in(&state, &user_info, PermissionsGroup::SomeStaff)
        .await
        .is_some()
    {
        return Ok(StatusCode::FORBIDDEN);
    }
    let user_info = user_info.unwrap();
    let note: Option<StaffNote> = sqlx::query_as(sql::GET_STAFF_NOTE)
        .bind(note_id)
        .fetch_optional(&state.db)
        .await?;
    if let Some(note) = note {
        if note.by == user_info.cid {
            sqlx::query(sql::DELETE_STAFF_NOTE)
                .bind(note_id)
                .execute(&state.db)
                .await?;
            info!("{} removed their note #{}", user_info.cid, note_id);
        }
    }
    Ok(StatusCode::OK)
}

/// Render a page snippet that shows training notes and a button to create more.
///
/// For training staff members.
async fn snippet_get_training_records(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(cid): Path<u32>,
) -> Result<Response, AppError> {
    use voca_rs::Voca;

    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if let Some(redirect) =
        reject_if_not_in(&state, &user_info, PermissionsGroup::TrainingTeam).await
    {
        return Ok(redirect.into_response());
    }
    let all_training_records = get_training_records(&state.config.vatsim.vatusa_api_key, cid)
        .await
        .map_err(|e| AppError::GenericFallback("getting VATUSA training records", e))?;
    let mut training_records: Vec<_> = all_training_records
        .iter()
        .filter(|record| record.facility_id == "ZDV")
        .map(|record| {
            let record = record.clone();
            TrainingRecord {
                notes: record.notes._strip_tags(),
                ..record
            }
        })
        .collect();

    // Sort by session_date in descending order (newest first)
    training_records.sort_by(|a, b| {
      let date_a = NaiveDateTime::parse_from_str(&a.session_date, "%Y-%m-%d %H:%M:%S").unwrap_or_else(|_| NaiveDateTime::from_timestamp(0, 0));
      let date_b = NaiveDateTime::parse_from_str(&b.session_date, "%Y-%m-%d %H:%M:%S").unwrap_or_else(|_| NaiveDateTime::from_timestamp(0, 0));
      date_b.cmp(&date_a) // Sort newest first
    });

    let instructor_cids: Vec<u32> = training_records
        .iter()
        .map(|record| record.instructor_id)
        .collect::<HashSet<u32>>()
        .iter()
        .copied()
        .collect();
    let instructors = get_multiple_controller_names(&instructor_cids).await;
    let template = state.templates.get_template("controller/training_notes")?;
    let rendered: String =
        template.render(context! { user_info, training_records, instructors })?;
    Ok(Html(rendered).into_response())
}

#[derive(Debug, Deserialize)]
struct NewTrainingRecordForm {
    date: String,
    duration: String,
    position: String,
    location: u8,
    notes: String,
    timezone: String,
}

/// Submit a new training note for the controller.
///
/// For training staff members.
async fn post_add_training_note(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(cid): Path<u32>,
    Form(record_form): Form<NewTrainingRecordForm>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if let Some(redirect) =
        reject_if_not_in(&state, &user_info, PermissionsGroup::TrainingTeam).await
    {
        return Ok(redirect);
    }
    let user_info = user_info.unwrap();
    let date = js_timestamp_to_utc(&record_form.date, &record_form.timezone)?;
    let new_record = NewTrainingRecord {
        instructor_id: format!("{}", user_info.cid),
        date,
        position: record_form.position,
        duration: record_form.duration,
        location: record_form.location,
        notes: record_form.notes,
    };
    match save_training_record(&state.config.vatsim.vatusa_api_key, cid, &new_record).await {
        Ok(_) => {
            flashed_messages::push_flashed_message(
                session,
                MessageLevel::Info,
                "New training record saved",
            )
            .await?;
            info!("{} submitted new training record for {cid}", user_info.cid);
        }
        Err(e) => {
            error!("Error saving new training record for {cid}: {e}");
            flashed_messages::push_flashed_message(
                session,
                MessageLevel::Error,
                "Could not save new training record",
            )
            .await?;
        }
    }

    Ok(Redirect::to(&format!("/controller/{cid}")))
}

/// Submit a form to change the controller's roles.
///
/// For admin staff members.
async fn post_set_roles(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(cid): Path<u32>,
    Form(roles_form): Form<HashMap<String, String>>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if let Some(redirect) = reject_if_not_in(&state, &user_info, PermissionsGroup::SomeStaff).await
    {
        return Ok(redirect);
    }
    let roles_can_set = roles_to_set(&state.db, &user_info).await?;
    let user_info = user_info.unwrap();
    let controller: Option<Controller> = sqlx::query_as(sql::GET_CONTROLLER_BY_CID)
        .bind(cid)
        .fetch_optional(&state.db)
        .await?;
    let controller = match controller {
        Some(c) => c,
        None => {
            warn!(
                "{} tried to set roles for unknown controller {cid}",
                user_info.cid
            );
            flashed_messages::push_flashed_message(
                session,
                MessageLevel::Error,
                "Unknown controller",
            )
            .await?;
            return Ok(Redirect::to(&format!("/controller/{cid}")));
        }
    };
    let existing_roles: Vec<_> = controller.roles.split_terminator(',').collect();
    let mut resolved_roles = Vec::new();
    let roles_to_set: Vec<_> = roles_form.keys().map(|s| s.as_str()).collect();

    // handle the form's data
    for role in existing_roles {
        if roles_can_set.contains(role) {
            if roles_to_set.contains(&role) {
                // if this user can set the role and it is still set, keep it
                resolved_roles.push(role);
            } else {
                // if this user can set the role and it no longer set, remove it
                // no-op
            }
        } else {
            // if this user cannot set the role, keep it
            resolved_roles.push(role);
        }
    }
    for role in &roles_to_set {
        // protection against form interception
        if roles_can_set.contains(*role) {
            resolved_roles.push(role);
        }
    }

    let new_roles = resolved_roles
        .iter()
        .collect::<HashSet<&&str>>()
        .iter()
        .join(",");

    info!(
        "{} is setting roles for {cid} to '{}'; was '{}'",
        user_info.cid, new_roles, controller.roles
    );
    sqlx::query(sql::SET_CONTROLLER_ROLES)
        .bind(cid)
        .bind(new_roles)
        .execute(&state.db)
        .await?;
    flashed_messages::push_flashed_message(session, MessageLevel::Info, "Roles updated").await?;

    Ok(Redirect::to(&format!("/controller/{cid}")))
}

pub fn router(templates: &mut Environment) -> Router<Arc<AppState>> {
    templates
        .add_template(
            "controller/controller",
            include_str!("../../templates/controller/controller.jinja"),
        )
        .unwrap();
    templates
        .add_template(
            "controller/training_notes",
            include_str!("../../templates/controller/training_notes.jinja"),
        )
        .unwrap();
    templates.add_function(
        "includes",
        |roles: Vec<String>, role: String| -> Result<bool, minijinja::Error> {
            Ok(roles.contains(&role))
        },
    );

    Router::new()
        .route("/controller/:cid", get(page_controller))
        .route("/controller/:cid/discord/unlink", post(api_unlink_discord))
        .route("/controller/:cid/ois", post(post_change_ois))
        .route("/controller/:cid/certs", post(post_change_certs))
        .route("/controller/:cid/note", post(post_new_staff_note))
        .route(
            "/controller/:cid/note/:note_id",
            delete(api_delete_staff_note),
        )
        .route(
            "/controller/:cid/training_records",
            get(snippet_get_training_records).post(post_add_training_note),
        )
        .route("/controller/:cid/roles", post(post_set_roles))
}
