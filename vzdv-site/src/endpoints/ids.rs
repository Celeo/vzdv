//! Endpoints for the integrated IDS.

use crate::shared::{AppError, AppState, SESSION_USER_INFO_KEY, UserInfo, reject_if_not_in};
use axum::{
    Router,
    extract::{Json as JsonE, State},
    response::{IntoResponse, Json as JsonR, Response},
    routing::{get, post},
};
use chrono::{TimeDelta, Utc};
use log::{debug, error};
use reqwest::StatusCode;
use std::sync::Arc;
use tower_sessions::Session;
use vzdv::sql::{self, AtisData};

/// Receive HTTP POST events from vATIS being ran by facility controllers.
///
/// Note that there doesn't seem to be a way to _authenticate_ that the data
/// is actually coming from vATIS ....
async fn receive_vatis_post(
    State(state): State<Arc<AppState>>,
    JsonE(payload): JsonE<AtisData>,
) -> Result<StatusCode, AppError> {
    let existing: Vec<AtisData> = sqlx::query_as(sql::GET_ALL_ATIS_ENTRIES)
        .fetch_all(&state.db)
        .await?;
    let now = Utc::now();
    let to_remove: Vec<_> = existing
        .iter()
        .filter(|entry| {
            let expired = (now - entry.timestamp) >= TimeDelta::hours(1);
            let matching =
                entry.facility == payload.facility && entry.atis_type == payload.atis_type;
            expired || matching
        })
        .map(|entry| entry.id)
        .collect();
    // can't use `.for_each` because of async
    for index in to_remove {
        if let Err(e) = sqlx::query(sql::DELETE_ATIS_ENTRY)
            .bind(index)
            .execute(&state.db)
            .await
        {
            error!("Could not delete stale ATIS {index}: {e}");
        }
    }
    sqlx::query(sql::INSERT_ATIS_ENTRY)
        .bind(serde_json::to_string(&payload)?)
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
    let data: Vec<AtisData> = sqlx::query_as(sql::GET_ALL_ATIS_ENTRIES)
        .fetch_all(&state.db)
        .await?;
    Ok(JsonR(data).into_response())
}

/// This file's routes and templates.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/ids/vatis/submit", post(receive_vatis_post))
        .route("/ids/vatis/current", get(show_atis_data))
}
