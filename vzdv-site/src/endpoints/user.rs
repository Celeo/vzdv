//! HTTP endpoints for user-specific pages.

use crate::{
    discord, flashed_messages,
    shared::{record_log, strip_some_tags, AppError, AppState, UserInfo, SESSION_USER_INFO_KEY},
};
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use chrono::NaiveDateTime;
use log::{debug, warn};
use minijinja::context;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tower_sessions::Session;
use vzdv::{
    sql::{self, Controller},
    vatusa::{self, get_multiple_controller_names, TrainingRecord},
};

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
    // let all_training_records = vatusa::get_training_records(user_info.cid, &state.config.vatsim.vatusa_api_key)
    let all_training_records =
        vatusa::get_training_records(1640903, &state.config.vatsim.vatusa_api_key)
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
    let instructor_cids: Vec<u32> = training_records
        .iter()
        .map(|record| record.instructor_id)
        .collect::<HashSet<u32>>()
        .iter()
        .copied()
        .collect();
    let instructors = get_multiple_controller_names(&instructor_cids).await;

    let template = state.templates.get_template("user/training_notes.jinja")?;
    let rendered = template.render(context! { user_info, training_records, instructors })?;
    Ok(Html(rendered).into_response())
}

/// Show the user a link to the Discord server, as well as provide
/// the start of the Discord OAuth flow for account linking.
async fn page_discord(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Response, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let user_info = match user_info {
        Some(info) => info,
        None => return Ok(Redirect::to("/").into_response()),
    };
    let controller: Controller = sqlx::query_as(sql::GET_CONTROLLER_BY_CID)
        .bind(user_info.cid)
        .fetch_one(&state.db)
        .await?;
    let template = state.templates.get_template("user/discord.jinja")?;
    let flashed_messages = flashed_messages::drain_flashed_messages(session).await?;
    let rendered: String = template.render(context! {
        user_info,
        oauth_link => discord::get_oauth_link(&state.config),
        join_link => &state.config.discord.join_link,
        discord_id => controller.discord_id,
        flashed_messages
    })?;
    Ok(Html(rendered).into_response())
}

/// Navigation from the Discord OAuth flow.
async fn page_discord_callback(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Redirect, AppError> {
    let user_info: Option<UserInfo> = session.get(SESSION_USER_INFO_KEY).await?;
    let user_info = match user_info {
        Some(info) => info,
        None => {
            warn!("Unknown user hit Discord link callback page");
            flashed_messages::push_flashed_message(
                session,
                flashed_messages::MessageLevel::Error,
                "Not logged in",
            )
            .await?;
            return Ok(Redirect::to("/"));
        }
    };
    if let Some(code) = params.get("code") {
        debug!("Getting Discord info in callback");
        let access_token = discord::code_to_token(code, &state.config).await?;
        let discord_user_id = discord::get_token_user_id(&access_token).await?;
        sqlx::query(sql::SET_CONTROLLER_DISCORD_ID)
            .bind(user_info.cid)
            .bind(&discord_user_id)
            .execute(&state.db)
            .await?;
        flashed_messages::push_flashed_message(
            session,
            flashed_messages::MessageLevel::Info,
            "Discord account linked",
        )
        .await?;
        record_log(
            format!(
                "Set Discord ID for controller {} to {}",
                user_info.cid, discord_user_id,
            ),
            &state.db,
            true,
        )
        .await?;
    } else {
        warn!(
            "Discord callback page hit by {} without code param",
            user_info.cid
        );
        flashed_messages::push_flashed_message(
            session,
            flashed_messages::MessageLevel::Error,
            "Could not link your Discord account - not enough info provided",
        )
        .await?;
    }
    Ok(Redirect::to("/user/discord"))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/user/training_notes", get(page_training_notes))
        .route("/user/discord", get(page_discord))
        .route("/user/discord/callback", get(page_discord_callback))
}
