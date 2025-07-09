//! Endpoints for the integrated IDS.

use crate::{
    flashed_messages,
    shared::{AppError, AppState, SESSION_USER_INFO_KEY, UserInfo, reject_if_not_in},
};
use axum::{
    Router,
    extract::{Json as JsonE, State},
    response::{Html, IntoResponse, Json as JsonR, Response},
    routing::{get, post},
};
use log::{debug, error};
use minijinja::context;
use reqwest::StatusCode;
use std::sync::Arc;
use tower_sessions::Session;
use vzdv::sql::{self, Atis};

/// Receive HTTP POST events from vATIS being ran by facility controllers.
///
/// Note that there doesn't seem to be a way to _authenticate_ that the data
/// is actually coming from vATIS ....
async fn receive_vatis_post(
    State(state): State<Arc<AppState>>,
    JsonE(payload): JsonE<Atis>,
) -> Result<StatusCode, AppError> {
    let existing: Vec<Atis> = sqlx::query_as(sql::GET_ALL_ATIS_ENTRIES)
        .fetch_all(&state.db)
        .await?;
    let matching: Vec<_> = existing
        .iter()
        .filter(|entry| entry.facility == payload.facility && entry.atis_type == payload.atis_type)
        .map(|entry| entry.id)
        .collect();
    // can't use `.for_each` because of async
    for index in matching {
        if let Err(e) = sqlx::query(sql::DELETE_ATIS_ENTRY)
            .bind(index)
            .execute(&state.db)
            .await
        {
            error!("Could not delete matching ATIS {index}: {e}");
        }
    }
    sqlx::query(sql::INSERT_ATIS_ENTRY)
        .bind(&payload.facility)
        .bind(&payload.preset)
        .bind(&payload.atis_letter)
        .bind(&payload.atis_type)
        .bind(&payload.airport_conditions)
        .bind(&payload.notams)
        .bind(payload.timestamp)
        .bind(&payload.version)
        .execute(&state.db)
        .await?;
    debug!("New ATIS data stored");
    Ok(StatusCode::OK)
}

async fn show_atis_data(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Response, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if let Some(redirect) =
        reject_if_not_in(&state, &user_info, vzdv::PermissionsGroup::LoggedIn).await
    {
        return Ok(redirect.into_response());
    }
    let data: Vec<Atis> = sqlx::query_as(sql::GET_ALL_ATIS_ENTRIES)
        .fetch_all(&state.db)
        .await?;
    Ok(JsonR(data).into_response())
}

/// Show the base IDS page.
async fn page_home(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Response, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    if let Some(redirect) =
        reject_if_not_in(&state, &user_info, vzdv::PermissionsGroup::LoggedIn).await
    {
        return Ok(redirect.into_response());
    }
    let template = state.templates.get_template("ids/base.jinja")?;
    let flashed_messages = flashed_messages::drain_flashed_messages(session).await?;
    let rendered = template.render(context! { user_info, flashed_messages, })?;
    Ok(Html(rendered).into_response())
}

/// This file's routes and templates.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/ids", get(page_home))
        .route("/ids/vatis/submit", post(receive_vatis_post))
        .route("/ids/vatis/current", get(show_atis_data))
}
