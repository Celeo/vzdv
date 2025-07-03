//! Endpoints for the integrated IDS.

use crate::shared::{
    AppError, AppState, SESSION_USER_INFO_KEY, UserInfo, VatisData, reject_if_not_in,
};
use axum::{
    Router,
    extract::{Json, State},
    response::{IntoResponse, Json as JsonResponse, Response},
    routing::{get, post},
};
use reqwest::StatusCode;
use std::sync::Arc;
use tower_sessions::Session;

/// Receive HTTP POST events from vATIS being ran by facility controllers.
async fn receive_vatis_post(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<VatisData>,
) -> Result<StatusCode, AppError> {
    let mut guard = state
        .atis_data
        .lock()
        .map_err(|_| AppError::MutexLockError)?;
    guard.retain(|data| {
        // keep values that don't match the incoming ATIS data
        !(data.facility == payload.facility && data.atis_type == payload.atis_type)
    });
    // add the new value
    guard.push(payload);
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

    let response = {
        let guard = match state.atis_data.try_lock() {
            Ok(g) => g,
            Err(e) => {
                return Ok((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Couldn't get mutex lock: {e:?}"),
                )
                    .into_response());
            }
        };
        serde_json::to_string(&*guard)?
    };

    Ok(JsonResponse(response).into_response())
}

/// This file's routes and templates.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/ids/vatis/submit", post(receive_vatis_post))
        .route("/ids/vatis/current", get(show_atis_data))
}
