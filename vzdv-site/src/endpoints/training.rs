use crate::{
    flashed_messages, load_templates,
    shared::{strip_some_tags, AppError, AppState, UserInfo, SESSION_USER_INFO_KEY},
};
use axum::{
    extract::State,
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use chrono::NaiveDateTime;
use minijinja::context;
use std::sync::Arc;
use tower_sessions::Session;
use vzdv::vatusa::{self, TrainingRecord};

async fn training_home(session: Session) -> Result<Response, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let user_info = match user_info {
        Some(ui) => ui,
        None => {
            return Ok(Redirect::to("/").into_response());
        }
    };
    let env = load_templates().unwrap();
    let template = env.get_template("training/home.jinja")?;
    let flashed_messages = flashed_messages::drain_flashed_messages(session).await?;
    let rendered = template.render(context! { user_info, flashed_messages })?;
    Ok(Html(rendered).into_response())
}
/// Retrieve and show the user their training records from VATUSA.
async fn page_training_notes(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Response, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let user_info = match user_info {
        Some(info) => info,
        None => return Ok(Redirect::to("/").into_response()),
    };
    let all_training_records =
        vatusa::get_training_records(&state.config.vatsim.vatusa_api_key, user_info.cid)
            .await
            .map_err(|e| {
                AppError::GenericFallback("getting VATUSA training records by controller", e)
            })?;
    let mut training_records: Vec<_> = all_training_records
        .iter()
        .filter(|record| record.facility_id == "ZDV")
        .map(|record| {
            let record = record.clone();
            TrainingRecord {
                notes: strip_some_tags(&record.notes).replace("\n", "<br>"),
                ..record
            }
        })
        .collect();

    // sort by session_date in descending order (newest first)
    training_records.sort_by(|a, b| {
        let date_a = NaiveDateTime::parse_from_str(&a.session_date, "%Y-%m-%d %H:%M:%S")
            .unwrap_or_else(|_| NaiveDateTime::default());
        let date_b = NaiveDateTime::parse_from_str(&b.session_date, "%Y-%m-%d %H:%M:%S")
            .unwrap_or_else(|_| NaiveDateTime::default());
        date_b.cmp(&date_a) // sort newest first
    });

    let template = state.templates.get_template("training/self_notes.jinja")?;
    let rendered = template.render(context! { user_info, training_records })?;
    Ok(Html(rendered).into_response())
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/training", get(training_home))
        .route("/training/my_notes", get(page_training_notes))
}
